# Gold Band DSL 概览

## 1. 一句话定义
Gold Band DSL 是一份面向 runtime 的最小工作流描述规范。

## 2. 当前主结构
- 节点：`worker / exec / verify`，其中新建工作流默认只生成 `worker`
- 边：顺序、分支、循环，可指向节点、`$end` 或 `$new-round`
- 默认策略：例如 `control.onAcceptanceFailure`、边级 `session` 与节点级 AI 输出验证

## 3. 当前设计原则
- provider-first
- 节点少于边界条件重要
- session 策略属于边，不属于节点
- edge 的 `session` 可省略；省略时默认 `new`
- AI 决定做什么，runtime 决定是否通过

## 4. 子文档结构
- [Control DSL](control.md)

### 节点规范
- [worker 节点](nodes/worker.md)
- [exec 节点](nodes/exec.md)
- [verify 节点](nodes/verify.md)

### 标准产物规范
- [exec-plan](artifacts/exec-plan.md)
- [exec-result](artifacts/exec-result.md)
- [verify-result](artifacts/verify-result.md)

## 5. canonical workflow 结构总览

首版建议将 workflow 的 canonical JSON 统一收敛为：

```json
{
  "version": "0.1",
  "id": "dev-test-verify",
  "entry": "dev",
  "control": {
    "maxRepairLoops": 3,
    "maxAcceptanceLoops": 2,
    "onAcceptanceFailure": "auto_loop"
  },
  "nodes": [
    {
      "id": "dev",
      "type": "worker",
      "provider": "claude-code",
      "profile": "pf-example-developer",
      "goal": "实现需求并给出执行计划",
      "primaryArtifact": "exec-plan"
    },
    {
      "id": "run-tests",
      "type": "exec",
      "planFrom": "dev"
    },
    {
      "id": "accept",
      "type": "verify"
    }
  ],
  "edges": [
    { "from": "dev", "to": "run-tests", "on": "success" },
    { "from": "run-tests", "to": "accept", "on": "success" },
    { "from": "run-tests", "to": "dev", "on": "failure", "session": "continue" }
  ]
}
```

### 顶层字段
- `version`：DSL 版本号，首版固定为 `0.1`
- `id`：workflow 标识
- `entry`：入口节点 id
- `control`：全局控制策略
- `nodes`：节点列表
- `edges`：边列表

### 节点类型
首版固定三类：
- `worker`
- `exec`
- `verify`

### 控制原则
- 小循环走 `edges`
- 大循环走 `control.onAcceptanceFailure`
- 旧版 `verify` 节点仍兼容，但新建工作流优先使用 worker 节点的 JSON 输出验证表达 review/test/accept
- edge 的 `to` 可指向节点 id、特殊终止目标 `"$end"`，或打开新 round 的 `"$new-round"`
- 新一轮 worker 的目标不改写原始 requirement，而是消费原始 requirement、前序反馈与运行时快照上下文
- `worker.goal` 是 runtime `taskInstruction` 的 canonical 来源，并进入 `userPrompt` 的 `# Task`
- `exec.invalid` 允许一条受限默认规则：若未显式声明 repair edge，可默认回到 `planFrom` 对应的 worker；默认优先使用 `continue`，若 provider 不支持则降级为 `new`

## 6. 当前关键结论
- DSL 当前使用 `worker` 作为默认 AI worker 节点名
- 执行层必须是 provider-first，而不是 Claude-only
- `worker` 节点必须显式声明 `provider`，桌面作者态 UI 从 Agent 管理页已配置 agent 中选择
- `profile` 保存 profile `id`，运行前按当前项目级 profile、用户级 profile 的优先级解析为可见角色正文；保存/运行前必须可见
- `worker` 一次只允许一个 `primaryArtifact`
- `worker.output.kind=json` + `successCondition` 可把 AI 输出字段转换为 `success / failure / invalid` 路由结果
- `failure` 与 `invalid` 同时保留，但边界不同：`failure` 表示目标未达成或执行失败，`invalid` 表示结果不满足最小 contract

## 7. 后续优先事项
- 继续细化节点输入契约
- 继续细化 edge / control 的完整字段集
- 把更多已知规则分拆进节点和 artifact 子文档