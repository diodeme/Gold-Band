# `node.json` 规范

## 1. 一句话定义
`node.json` 保存某个节点一次 attempt 的执行元信息。

它用于表达：
- 这是哪个 node 的哪一次 attempt
- 当前 attempt 的状态和 outcome 是什么
- 这次 attempt 解析后的关键配置是什么
- 它和 `worker-ref.json`、canonical artifacts 如何关联

---

## 2. 最小结构

```json
{
  "version": "0.1",
  "acp_storage_schema_version": 2,
  "nodeId": "dev",
  "nodeType": "worker",
  "runId": "run-001",
  "roundId": "round-001",
  "attemptId": "attempt-002",
  "status": "completed",
  "outcome": "success",
  "startedAt": "2026-03-20T10:31:00Z",
  "finishedAt": "2026-03-20T10:31:45Z",
  "resolvedConfig": {
    "provider": "claude-code",
    "profile": "developer",
    "outputArtifact": "dev-result",
    "sessionMode": "new"
  }
}
```

---

## 3. 必填字段
- `version`
- `acp_storage_schema_version`
- `nodeId`
- `nodeType`
- `runId`
- `roundId`
- `attemptId`
- `status`
- `outcome`
- `startedAt`
- `resolvedConfig`

条件必填：
- `finishedAt`：当 `status = completed` 时必须存在

---

## 4. 字段说明

### `acp_storage_schema_version`

- 类型：非负整数
- 当前版本：`2`
- 归属：attempt 内 ACP durable storage 的 canonical schema version

该字段与 `version`、`acp.timeline.index.json.formatVersion` 分属不同领域：`version` 描述 `node.json` 业务状态结构，`acp_storage_schema_version` 描述当前 attempt 的 ACP 存储布局，Timeline index 的 `formatVersion` 只描述可删除、可重建的派生索引格式。

新 attempt 在首次写入 `node.json` 时直接声明当前版本，因此正常启动不得运行 legacy Timeline/Agent result 迁移。旧 `node.json` 缺少该字段时按 `0` 读取，并按 `0 → 1 → 2` 顺序执行幂等迁移；每一步数据改写成功后才原子推进该字段，失败时保留上一步版本以供重试。高于当前实现的版本必须拒绝打开，不能按旧格式猜测读取。

历史 `.acp-branch-timeline-migration-v1` 与 `.acp-agent-result-migration-v2` 不再属于任何判断或清理路径：已有文件原样保留并被忽略，新 attempt 不再创建。生命周期写回 `node.json` 时必须保留已经落盘的更高 ACP schema version，迟到的旧 `NodeState` 不得使版本回退。

路径与字段命名由现有状态领域决定：

- 普通 Workflow/Direct attempt 以 `rounds/<round>/nodes/<node>/<attempt>/node.json.acp_storage_schema_version` 为事实源。
- AI-DYNAMIC leaf 当前固定使用 `attempt-001`，其 ACP 历史位于 `dynamic/nodes/<leaf>/attempt-001/`，版本事实记录在父级 leaf 状态 `dynamic/nodes/<leaf>/node.json.acpStorageSchemaVersion`；attempt 目录内不再增加一份 manifest。
- `dynamic/graph.json.nodes` 不持久化该字段。graph 负责拓扑和聚合，leaf `node.json` 负责该 leaf 的 ACP attempt 存储版本，避免同一版本出现两个可独立写回的 durable 副本。

两种路径共享同一个版本常量、`0 → 1 → 2` 迁移步骤和单调写入契约。AI-DYNAMIC 生命周期从 `graph.json` 加载出的内存节点不携带该版本；写回 leaf `node.json` 时必须在文件锁内保留磁盘已提交的更高版本。

### `nodeType`
- 类型：string
- 枚举：`worker | worker | output validation`

### `status`
- 类型：string
- 枚举：`running | paused | completed`

### `outcome`
- 类型：string | null
- 枚举：`success | failure | invalid | killed | null`

说明：
- `running` 时必须 `outcome = null`
- `paused` 时必须 `outcome = null`
- `completed` 时应为 `success | failure | invalid | killed`
- `paused` 只属于 `status`，不属于 `outcome`
- `failure` 表示目标未达成或执行失败
- `invalid` 表示结果不满足最小 contract

### `resolvedConfig`
- 类型：object
- 含义：本次 attempt 解析后的关键配置快照
- 该对象的内部字段可按 `nodeType` 不同而不同

#### 对 `worker`
建议至少可包含：
- `provider`
- `profile`
- `outputArtifact`
- `sessionMode`（例如 `new | continue`）

#### 对 `worker`
建议至少可包含：
- `显式 edge`

#### 对 `worker`
建议至少可包含：
- `provider`
- `profile`
- `outputArtifact`（固定为 `验收输出产物`）
- `failure 边`
- `evidenceScope`（首版固定为 `current-round`）

说明：
- 虽然 `worker` 在 DSL 上是独立节点类型，但在执行层复用 provider worker 通道
- 因此 `worker` 的 `resolvedConfig` 建议保留与 `worker` 对称的 provider/profile 信息

---

## 5. runtime 校验规则
以下情况应视为 `invalid`：

- 缺少任一必填字段
- `nodeType` 不在合法枚举内
- `status` 不在合法枚举内
- `outcome` 不在合法枚举内且不为 null
- `status = running` 但 `outcome != null`
- `status = paused` 但 `outcome != null`
- `status = completed` 但 `outcome = null`
- `status = completed` 但缺少 `finishedAt`
- `resolvedConfig` 不是对象
- `acp_storage_schema_version` 高于当前实现支持的版本

---

## 6. 与同目录文件的关系
同一个 attempt 目录下，`node.json` 与这些文件协同工作：

- `worker-ref.json`
- `artifacts/`
- `attachments/`

其中：
- `node.json` 记录 attempt 元信息
- `node.json.acp_storage_schema_version` 记录 attempt 的 ACP 存储布局版本
- `worker-ref.json` 记录 provider-specific 会话引用
- `artifacts/` 保存 canonical artifacts

---

## 7. 相关文档
- [Runtime 概览](../overview.md)
- [Worker Ref 规范](../../provider/worker-ref.md)
- [Worker Invocation Contract](../../provider/invocation.md)

---

## 8. 一句话总结

> `node.json` 是 attempt 级元信息快照：它告诉 runtime 当前这个节点这次是怎么跑的、跑成什么状态，以及它解析后的关键配置是什么。
