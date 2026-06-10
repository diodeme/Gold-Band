# 客户端上报接口接入指南

> 面向客户端开发者。Console 提供两个上报接口：**心跳上报**（活跃统计）和**节点指标上报**（任务运行指标）。两个接口均不依赖 Console 登录态，使用专用 API Key 鉴权。

---

## 0. 本次分配的 API Key

```
hMfUAuGbxO1mNYruf80z00O24GmoSga2eAxlS_bK7zQ
```

- **Header 名**：`X-Maling-Report-Key`
- **作用范围**：心跳上报 + 节点指标上报 两个接口共用
- **存放方式**：从客户端的配置文件 / 环境变量读取，**禁止**硬编码到发布包源码，**禁止**写入任何日志
- **泄露处理**：立即联系 Console 运维轮换
- **轮换通知**：运维方更换 Key 时会提前在群里通知；客户端需支持热更新或重启加载

> ⚠️ 本 Key 仅限本项目客户端使用，请勿外传或用于其它系统。

---

## 1. 鉴权约定

### 1.1 请求头

所有上报请求必须携带以下 HTTP Header：

| Header | 说明 | 是否必填 |
|---|---|---|
| `X-Maling-Report-Key` | 服务端预共享的 API Key（区分大小写，原样传递） | 是 |
| `Content-Type` | 固定 `application/json;charset=UTF-8` | 是 |

> Header 名称由服务端 `client-report.api-key-header` 配置，默认 `X-Maling-Report-Key`。如运维侧更名，会同步通知。

### 1.2 鉴权失败行为

- 未携带 Key、Key 为空字符串、Key 长度不一致、Key 内容不匹配 → 一律返回 `code = 403`，**不写入任何数据**
- 服务端运维通过开关关闭上报功能 → 一律返回 `code = 403, msg = "Client report is disabled"`

### 1.3 Key 安全要求

- 不得将 Key 硬编码到客户端发布包；从配置文件、环境变量或运行时下发渠道读取
- 不得打印到任何日志（包括 debug 日志）
- 服务端会定期或应急轮换 Key，客户端需要支持热更新/重启加载

---

## 2. 通用响应结构

所有接口返回统一 `R<T>` 包装：

```json
{
  "code": 200,
  "msg": "",
  "ok": true,
  "data": { /* 具体业务字段，见各接口说明 */ }
}
```

| 字段 | 类型 | 说明 |
|---|---|---|
| `code` | int | 200=成功；403=鉴权失败；400=参数错误；500=服务端错误 |
| `msg` | string | 错误描述；成功时为空 |
| `ok` | boolean | 等价于 `code == 200` |
| `data` | object/null | 业务数据 |

**客户端判定成功的标准**：HTTP 状态码 200 **且** `code == 200`。

---

## 3. 心跳上报接口

### 3.1 基本信息

| 项 | 值 |
|---|---|
| URL | `POST /api/client-report/heartbeat` |
| Content-Type | `application/json;charset=UTF-8` |
| 鉴权 | `X-Maling-Report-Key` |
| 调用频率建议 | 每 1~5 分钟一次（用户活跃时） |

### 3.2 请求体字段

| 字段 | 类型 | 必填 | 长度限制 | 说明 |
|---|---|---|---|---|
| `userId` | string | 是 | <=128 | 客户端用户唯一标识（去重统计的主键，建议使用稳定 ID，不要用易变的昵称） |
| `clientVersion` | string | 否 | <=64 | 客户端版本号，便于按版本分析 |
| `reportedAt` | string | 否 | ISO-8601 | 客户端上报时间；缺省服务端用当前时间填充 |

### 3.3 请求示例

```http
POST /api/client-report/heartbeat HTTP/1.1
Host: console.example.com
Content-Type: application/json;charset=UTF-8
X-Maling-Report-Key: hMfUAuGbxO1mNYruf80z00O24GmoSga2eAxlS_bK7zQ

{
  "userId": "u_2024abc",
  "clientVersion": "1.4.2",
  "reportedAt": "2026-06-06T10:23:45"
}
```

### 3.4 响应示例

成功：
```json
{
  "code": 200,
  "msg": "",
  "ok": true,
  "data": {
    "accepted": true,
    "receivedAt": "2026-06-06T10:23:45.812"
  }
}
```

鉴权失败：
```json
{
  "code": 403,
  "msg": "Invalid client report api key",
  "ok": false,
  "data": null
}
```

### 3.5 客户端实现建议

1. **去重交给服务端**：同一用户一天可以心跳几十次，DAU 聚合天然去重，客户端不要为了"省请求"自行节流到一天一次。
2. **失败可丢弃**：心跳是统计指标，丢一两个对结果无影响；不需要本地持久化重试队列。
3. **建议在以下时机上报**：客户端启动、用户操作（点击/打字）后的 5 分钟节流窗口、应用从后台切回前台。
4. **不要在客户端长时间空闲时持续心跳**——会污染 DAU 数据。

---

## 4. 节点指标上报接口

### 4.1 基本信息

| 项 | 值 |
|---|---|
| URL | `POST /api/client-report/metrics/batch` |
| Content-Type | `application/json;charset=UTF-8` |
| 鉴权 | `X-Maling-Report-Key` |
| 单次最大条数 | 1000 |

### 4.2 业务模型

```
workspace
 └─ user (userId)
     └─ task (taskId)
         └─ run (runId)              一个 task 可以多次执行
             └─ round (roundId)      一次 run 内有多轮
                 └─ node (nodeId)    每轮内有多个节点
```

**节点的业务唯一键**：`(workspace, userId, taskId, runId, roundId, nodeId)`。

服务端按这个键做 **upsert**：第一次见到该节点 → INSERT；后续重复上报 → UPDATE 全字段为最新值。

### 4.3 请求体字段

```jsonc
{
  "metrics": [
    {
      // === 业务键 (必填，6 个字段共同标识一个节点) ===
      "workspace": "ws-prod",
      "userId":    "u_2024abc",
      "taskId":    "task-001",
      "runId":     "run-20260606-001",
      "roundId":   "round-1",
      "nodeId":    "node-7",

      // === 节点属性 (可选) ===
      "seq":          7,            // 节点在 round 内的序号
      "agentType":    "code-search",// agent 类型
      "attemptCount": 1,            // 重试次数，缺省 0

      // === 时间 (可选；ISO-8601；用于平均耗时统计) ===
      "startedAt":  "2026-06-06T10:21:00",
      "endedAt":    "2026-06-06T10:21:12",

      // === Token 计数 (可选，缺省全部 0) ===
      "inputTokens":     1200,
      "outputTokens":    340,
      "cacheReadTokens": 800,
      "totalTokens":     1540,

      // === 状态 (必填) ===
      "status": "SUCCESS",

      // === 上报时间 (可选；缺省服务端用 now()) ===
      "reportedAt": "2026-06-06T10:21:13"
    }
  ]
}
```

### 4.4 字段约束表

| 字段 | 类型 | 必填 | 约束 | 说明 |
|---|---|---|---|---|
| `metrics` | array | 是 | 长度 1~1000 | 节点指标数组；空数组拒绝 |
| `metrics[].workspace` | string | 是 | <=255 | 工作空间名 |
| `metrics[].userId` | string | 是 | <=128 | 与心跳的 userId 保持一致 |
| `metrics[].taskId` | string | 是 | <=128 | task 标识 |
| `metrics[].runId` | string | 是 | <=128 | run 标识 |
| `metrics[].roundId` | string | 是 | <=128 | round 标识 |
| `metrics[].nodeId` | string | 是 | <=128 | node 标识 |
| `metrics[].seq` | int | 否 | >=0 | 节点序号 |
| `metrics[].agentType` | string | 否 | <=64 | agent 类型 |
| `metrics[].attemptCount` | int | 否 | >=0 | 重试次数 |
| `metrics[].startedAt` | string | 否 | ISO-8601 | 节点开始时间 |
| `metrics[].endedAt` | string | 否 | ISO-8601 | 节点结束时间 |
| `metrics[].inputTokens` | long | 否 | >=0 | 输入 token |
| `metrics[].outputTokens` | long | 否 | >=0 | 输出 token |
| `metrics[].cacheReadTokens` | long | 否 | >=0 | 缓存读取 token |
| `metrics[].totalTokens` | long | 否 | >=0 | token 总和（建议客户端计算后传入；服务端不会自动合计前三项） |
| `metrics[].status` | string | 是 | 严格大写 | 必须为 `RUNNING` / `SUCCESS` / `FAILED` / `PAUSED` 之一 |
| `metrics[].reportedAt` | string | 否 | ISO-8601 | 客户端上报时间 |

### 4.5 节点状态枚举

| 状态 | 含义 | 进入成功率分母 |
|---|---|---|
| `RUNNING` | 节点运行中 | 否 |
| `SUCCESS` | 节点执行成功 | 是 |
| `FAILED` | 节点执行失败 | 是 |
| `PAUSED` | 节点暂停 | 否 |

> ⚠️ 状态值必须**严格大写**。`success`、`Success` 等都会被拒绝（整批 400）。

> 成功率公式：`SUCCESS / (SUCCESS + FAILED)`。

### 4.6 完整请求示例

```http
POST /api/client-report/metrics/batch HTTP/1.1
Host: console.example.com
Content-Type: application/json;charset=UTF-8
X-Maling-Report-Key: hMfUAuGbxO1mNYruf80z00O24GmoSga2eAxlS_bK7zQ

{
  "metrics": [
    {
      "workspace": "ws-prod",
      "userId":    "u_2024abc",
      "taskId":    "task-001",
      "runId":     "run-20260606-001",
      "roundId":   "round-1",
      "nodeId":    "node-A",
      "seq": 1,
      "agentType": "planner",
      "attemptCount": 1,
      "startedAt": "2026-06-06T10:20:00",
      "endedAt":   "2026-06-06T10:20:05",
      "inputTokens": 800,
      "outputTokens": 200,
      "cacheReadTokens": 0,
      "totalTokens": 1000,
      "status": "SUCCESS"
    },
    {
      "workspace": "ws-prod",
      "userId":    "u_2024abc",
      "taskId":    "task-001",
      "runId":     "run-20260606-001",
      "roundId":   "round-1",
      "nodeId":    "node-B",
      "seq": 2,
      "agentType": "code-search",
      "attemptCount": 1,
      "startedAt": "2026-06-06T10:20:05",
      "inputTokens": 0,
      "outputTokens": 0,
      "cacheReadTokens": 0,
      "totalTokens": 0,
      "status": "RUNNING"
    }
  ]
}
```

### 4.7 响应示例

成功：
```json
{
  "code": 200,
  "msg": "",
  "ok": true,
  "data": {
    "acceptedCount": 2,
    "insertedOrUpdatedCount": 3,
    "receivedAt": "2026-06-06T10:20:06.214"
  }
}
```

- `acceptedCount`：本次请求接收的节点条数（=请求里 metrics 数组长度）
- `insertedOrUpdatedCount`：MySQL 受影响行数累计；INSERT 计 1，UPDATE 计 2

参数错误（如包含非法状态）：
```json
{
  "code": 400,
  "msg": "Invalid node status: success",
  "ok": false,
  "data": null
}
```

> ⚠️ **整批事务**：批内任一节点 `status` 非法、必填字段缺失 → 整批回滚，**一条都不写**。客户端如果希望部分成功，需要自己分批重试。

### 4.8 客户端实现建议

1. **节点状态变化时上报**：在节点进入 RUNNING、转为 SUCCESS/FAILED、被 PAUSED 时各上报一次。RUNNING 状态可以在 round 结束前重复上报，最后一次为准。
2. **批量发送优化**：建议本地维护一个缓冲队列，每 5~10 秒或攒到 50 条时打包发送，降低 QPS。
3. **网络失败重试**：可重试 2~3 次，间隔 1s/3s/10s 指数退避。最终失败可丢弃（统计场景容忍丢失）。
4. **同一节点重复上报**：服务端按 6 字段业务键做 upsert，客户端可以放心地"频繁上报最新状态"，不需要担心生成重复记录。
5. **时间字段**：建议传 ISO-8601 不带时区（`yyyy-MM-ddTHH:mm:ss`），服务端按本地时区解析。如服务端跨时区部署需要协调。
6. **Token 总量**：`totalTokens` 由客户端按业务口径自行计算（通常 = input + output + cacheRead，但取决于你们模型计费规则）。服务端不会校验三者之和。
7. **不要在请求间隔很短时上报相同字段**：upsert 写入虽然幂等，但会触发 InnoDB 的 update，浪费 IO。RUNNING 状态可以节流到 30 秒上报一次最新值。

---

## 5. 常见错误码

| HTTP | code | msg 模式 | 处理建议 |
|---|---|---|---|
| 200 | 200 | "" | 成功 |
| 200 | 400 | `xxx is required` / `xxx must be...` | 校验请求体字段；不重试 |
| 200 | 400 | `Invalid node status: xxx` | 客户端代码 bug，状态值不在四类枚举中；不重试 |
| 200 | 403 | `Client report is disabled` | 服务端关闭了上报；停止上报，告警 |
| 200 | 403 | `Invalid client report api key` | API Key 未配置/错误/失效；检查配置；不重试 |
| 5xx | - | - | 服务端异常；重试（指数退避） |

---

## 6. 排查工具

如怀疑上报数据未生效，可向运维侧请求：

```sql
-- 心跳是否进库
SELECT user_id, client_version, reported_at, received_at
FROM ml_client_heartbeat
WHERE user_id = 'u_2024abc'
ORDER BY reported_at DESC
LIMIT 10;

-- 节点指标是否进库
SELECT workspace, user_id, task_id, run_id, round_id, node_id,
       status, total_tokens, started_at, ended_at, reported_at, update_time
FROM ml_client_node_metric
WHERE user_id = 'u_2024abc' AND task_id = 'task-001'
ORDER BY update_time DESC
LIMIT 50;
```

提供 `requestId` 给后端运维可以快速定位日志。

---

## 7. 联系方式

- 接口异常 / 数据不一致：Console 后端团队
- API Key 申请 / 轮换：Console 运维
- 统计看板访问：Console 管理员 → 登录 Console → 侧边栏「客户端统计」（仅管理员可见）
