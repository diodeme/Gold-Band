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
    "primaryArtifact": "exec-plan",
    "sessionMode": "new"
  }
}
```

---

## 3. 必填字段
- `version`
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

### `nodeType`
- 类型：string
- 枚举：`worker | exec | verify`

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
- `primaryArtifact`
- `sessionMode`（例如 `new | continue`）

#### 对 `exec`
建议至少可包含：
- `planFrom`

#### 对 `verify`
建议至少可包含：
- `provider`
- `profile`
- `primaryArtifact`（固定为 `verify-result`）
- `onAcceptanceFailure`
- `evidenceScope`（首版固定为 `current-round`）

说明：
- 虽然 `verify` 在 DSL 上是独立节点类型，但在执行层复用 provider worker 通道
- 因此 `verify` 的 `resolvedConfig` 建议保留与 `worker` 对称的 provider/profile 信息

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

---

## 6. 与同目录文件的关系
同一个 attempt 目录下，`node.json` 与这些文件协同工作：

- `worker-ref.json`
- `artifacts/`
- `attachments/`

其中：
- `node.json` 记录 attempt 元信息
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