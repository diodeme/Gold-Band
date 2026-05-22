# Gold Band DSL 概览

## 1. 一句话定义
Gold Band DSL 是一份面向 runtime 的最小工作流描述规范：节点统一为 `worker`，结果判定由节点配置和边控制表达。

## 2. 当前主结构
- 节点：只有 `worker`。
- 边：顺序、分支、循环，可指向节点、`$end` 或 `$new-round`。
- 节点能力：`goal`、`provider`、`profile`、`primary_artifact`、`output`、`success_condition`、`manual_check`、`permission_mode`。
- 结果判定：默认 provider 成功即节点成功；开启 AI 输出验证时由 `output` + `success_condition` 判定；开启人工 check 时由用户确认 success/failure。

## 3. 当前设计原则
- provider-first：节点只声明使用哪个 agent/provider，不绑定具体实现。
- 数据优先：节点输出、结果判定和边跳转都显式落在 DSL 中。
- session 策略属于边，不属于节点；edge 的 `session` 可省略，默认 `new`。
- AI 决定做什么，runtime 只负责保存产物并把结果归纳为 `success / failure / invalid`。

## 4. 子文档结构
- [Control DSL](control.md)
- [worker 节点](nodes/worker.md)

输出产物不再按内置名称区分；每个 worker 通过 `primary_artifact` 或 `output.artifact` 自定义产物 key。

## 5. canonical workflow 示例

```json
{
  "version": "0.1",
  "id": "dev-test-accept",
  "entry": "dev",
  "control": { "max_attempts": 3, "max_rounds": 2 },
  "nodes": [
    {
      "id": "dev",
      "type": "worker",
      "provider": "claude-code",
      "profile": "pf-example-developer",
      "goal": "实现需求",
      "primary_artifact": "implementation-result"
    },
    {
      "id": "test",
      "type": "worker",
      "provider": "claude-code",
      "profile": "pf-example-tester",
      "goal": "检查实现并输出 JSON 结果",
      "primary_artifact": "test-result",
      "output": {
        "kind": "json",
        "artifact": "test-result",
        "schema": { "reason": "String", "result": "boolean" }
      },
      "success_condition": { "expression": "$.result == true" }
    },
    {
      "id": "accept",
      "type": "worker",
      "provider": "claude-code",
      "profile": "pf-example-acceptance",
      "goal": "对照需求判断是否满足验收条件",
      "manual_check": true
    }
  ],
  "edges": [
    { "from": "dev", "to": "test", "on": "success" },
    { "from": "test", "to": "accept", "on": "success" },
    { "from": "test", "to": "dev", "on": "failure", "session": "continue" },
    { "from": "accept", "to": "$new-round", "on": "failure" },
    { "from": "accept", "to": "$end", "on": "success" }
  ]
}
```

## 6. 关键约束
- `version` 首版固定为 `0.1`。
- `entry` 必须指向真实 worker 节点。
- `$end` 只能作为边目标，不能作为节点 id；`invalid -> $end` 非法。
- `session=continue` 只能指向真实 worker 节点，并且目标 provider 必须支持 continue session。
- `control.max_attempts` / `control.max_rounds` 都是可选正整数；不填表示不限制。
- `max_attempts` 按当前 round 内的 `来源节点 -> 目标节点` 分别计数，`max_rounds` 只统计 `$new-round` 打开的新 round。
- 同一来源节点的同一结果类型只能有一条出边，例如一个节点只能配置一条 `success` 边。
- `manual_check=true` 与 AI 输出验证互斥。
- `success_condition` 必须搭配 JSON `output` 使用。
- `output.artifact` 必须与 `primary_artifact` 一致。
- `output.schema` 使用简化输出结构，不使用 JSON Schema。

## 7. 结果语义
- `success`：节点完成且满足默认成功条件、AI 输出验证或人工 check。
- `failure`：节点完成但目标未达成，或 AI 输出验证返回 false。
- `invalid`：节点产物缺失、输出结构不合法，或成功条件无法按声明路径求值。
