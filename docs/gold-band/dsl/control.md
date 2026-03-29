# Gold Band Control DSL 规范

## 1. 一句话定义
Control DSL 用来定义 workflow 的**控制语法**，也就是：

- 节点之间怎么连
- 小循环怎么回
- 大循环怎么回
- 全局控制策略是什么
- runtime 启动前要校验哪些合法性条件

它描述的是 workflow 的控制面，不是运行产物本身。

---

## 2. 设计原则

### 2.1 控制语法属于 DSL，不属于 artifact
`exec-plan`、`exec-result`、`verify-result` 是运行产物。

但：
- 小循环如何回跳
- 大循环如何回跳
- session policy 是 `continue` 还是 `new`
- retry / loop 次数上限

这些都属于 workflow 控制语法，应放在 DSL 层表达。

### 2.2 显式优先，不做隐式推导
控制相关关键行为应尽量显式表达。

例如：
- `worker` 需要产出什么 primary artifact
- `exec` 消费哪个节点的 `exec-plan`
- 大循环失败怎么处理

runtime 可以做校验，但不应靠大量隐式推导替用户补语义。

### 2.3 内置节点名保留字必须受限
当前内置节点类型为：
- `worker`
- `exec`
- `verify`

为了避免歧义：
- 节点 `id` 不应直接使用这些保留字作为业务节点名
- 尤其不建议把某个自定义 `worker` 节点直接命名为 `exec` 或 `verify`

---

## 3. 控制 DSL 应表达的内容
首版建议至少表达以下几类信息：

### 3.1 全局控制配置
例如：
- 大循环最大次数
- 小循环最大次数
- `verify` 失败后的默认处理策略

### 3.2 节点定义
例如：
- `worker`
- `exec`
- `verify`

### 3.3 边定义
例如：
- 正常顺序流转
- 小循环回跳
- 大循环回跳
- session policy

### 3.4 启动前校验规则
例如：
- DSL 语法是否合法
- `exec` 前置是否真的声明了 `exec-plan`
- 节点引用是否存在
- 控制策略是否冲突

---

## 4. 顶层结构
建议在 workflow 顶层增加一个明确的 `control` 字段：

```json
{
  "version": "0.1",
  "id": "implement-feature",
  "entry": "dev",
  "control": {
    "maxRepairLoops": 3,
    "maxAcceptanceLoops": 2,
    "onAcceptanceFailure": "auto_loop"
  },
  "nodes": [],
  "edges": []
}
```

---

## 5. 全局控制配置

## 5.1 `control.maxRepairLoops`
- 类型：number
- 含义：单个 round 内，小循环允许的最大次数
- 适用场景：`exec.failure -> worker`

### 建议规则
- 必须为正整数
- 到达上限后，应由 runtime 结束当前 repair 路径
- 首版不建议允许无限循环

### 计数口径
`maxRepairLoops` 统计的不是 attempt 数，也不是完整闭环数；它统计的是：

> **在同一个 round 内，控制层因 `exec.failure` 或 `exec.invalid`，决定沿 repair 路径回到某个 `worker` 的次数。**

也就是说，计数发生在：
- `exec` 已产出 outcome
- runtime 决定“回到某个 `worker` 修复”
- 此次 repair 回跳真正成立

首版建议：
- `exec.failure -> worker`：计入 repair loop
- `exec.invalid -> worker`：计入 repair loop
- `worker.failure`：不计入 repair loop
- `worker.invalid`：不计入 repair loop
- `verify.failure`：不计入 repair loop
- `verify.invalid`：不计入 repair loop

## 5.2 `control.maxAcceptanceLoops`
- 类型：number
- 含义：单个 run 内，大循环允许的最大次数
- 适用场景：`verify.failure -> worker`

### 建议规则
- 必须为正整数
- 到达上限后，不再继续自动进入新 round

### 计数口径
`maxAcceptanceLoops` 统计的不是 round 总数；它统计的是：

> **在同一个 run 内，控制层因 `verify.failure`，决定新建 round 并回到 `workflow.entry` 的次数。**

也就是说，计数发生在：
- `verify` 已产出 `failure`
- runtime 决定进入下一轮
- 新 round 真正被创建

首版建议：
- `round-001` 是初始执行，不计入 acceptance loop
- `verify.failure + auto_loop`：在新 round 被创建时计入 acceptance loop
- `verify.failure + stop`：不计入 acceptance loop
- `verify.invalid`：不计入 acceptance loop

## 5.3 `control.onAcceptanceFailure`
- 类型：string
- 枚举：`auto_loop | stop`

### 语义
- `auto_loop`：自动进入下一轮，并回到 `workflow.entry`
- `stop`：直接失败结束

补充规则：
- 新一轮不会改写 task 的原始 requirement
- 下一轮 `worker` 应直接消费原始 requirement 与最新 `verify-result`

说明：
- 它是全局默认的验收失败处理策略
- 首版不建议再为大循环额外声明 `target` 或 `session`
- 大循环的回跳目标固定为 `workflow.entry`

---

## 6. 节点定义中的控制相关字段

## 6.1 `worker`
`worker` 节点建议显式声明：

- `provider`
- `profile`
- `goal`
- `primaryArtifact`

当前建议：
- `primaryArtifact` 直接表达该节点这次唯一标准输出的逻辑名
- 首版一个 `worker` 节点一次只应有一个 `primaryArtifact`
- `primaryArtifact` 是可选字段；未声明时，runtime 不要求 canonical artifact，而只依据 provider invocation 的完成状态归纳 `success / failure / paused`
- 未声明 `primaryArtifact` 时，只有 provider adapter 返回包本身不合法，runtime 才归为 `invalid`

示例：

```json
{
  "id": "test",
  "type": "worker",
  "provider": "claude-code",
  "profile": "tester",
  "goal": "编写测试并给出执行计划",
  "primaryArtifact": "exec-plan"
}
```

## 6.2 `exec`
`exec` 节点建议显式声明它消费哪个节点的 `exec-plan`。

示例：

```json
{
  "id": "run-tests",
  "type": "exec",
  "planFrom": "test"
}
```

说明：
- `planFrom` 必须指向某个 `worker` 节点
- 且该节点必须显式声明 `primaryArtifact = "exec-plan"`
- runtime 解析时只看当前 round 内该节点最新一次 attempt 的 `exec-plan`

## 6.3 `verify`
`verify` 节点是可选的最终验收关口。

首版建议：
- 一个 workflow 最多只能有一个 `verify`
- `onAcceptanceFailure` 只有在存在 `verify` 节点时才有效
- 若不存在 `verify` 节点却声明了 `onAcceptanceFailure`，应视为 DSL 校验错误

---

## 7. 边语法
边建议统一采用结构化对象，而不是只靠位置推断。

最小示意：

```json
{
  "from": "dev",
  "to": "run-tests",
  "on": "success",
  "session": "continue"
}
```

### 7.1 `from`
- 起点节点 id

### 7.2 `to`
- 终点节点 id
- 若需要语法糖，可在更上层支持 `A -> B.new`，但 canonical JSON 中建议仍展开为显式 `session` 字段

### 7.3 `on`
- 枚举建议：`success | failure | invalid`
- 表示当前 edge 在什么 outcome 下生效

### 7.4 `session`
- 枚举：`continue | new`
- 仅在回到 `worker` 节点时有明显意义

说明：
- `A -> B` 等价于 `session = continue` 这种语法糖，建议只出现在讨论层
- canonical JSON 仍建议显式写成结构化字段

---

## 8. 小循环与大循环的控制语法

## 8.1 小循环
典型写法：

```json
{
  "from": "run-tests",
  "to": "dev",
  "on": "failure",
  "session": "continue"
}
```

语义：
- `exec.failure` 回到 `worker`
- 不开新 round
- session 策略按 edge 明确声明
- 每当控制层实际沿这条 repair 路径回到 `worker` 一次，就消耗 1 次 repair loop 配额

## 8.2 大循环
大循环不通过普通 edge 表达，而由全局 `control.onAcceptanceFailure` 统一控制。

语义：
- `verify.failure` 后是否进入下一轮，只看：
  - `control.onAcceptanceFailure`
  - 当前 acceptance loop 次数是否已达上限
- 若 `onAcceptanceFailure = auto_loop`，则新建 round，并回到 `workflow.entry`
- 若 `onAcceptanceFailure = stop`，则直接结束 run
- 进入下一轮时，原始 requirement 保持不变；反馈直接来自最新 `verify-result`
- 每当控制层实际创建一个新的 acceptance round，就消耗 1 次 acceptance loop 配额

---

## 9. 启动前 DSL 合法性校验
runtime 在启动 control 前，应至少检查以下几类合法性。

## 9.1 语法合法性
- JSON 是否可解析
- 必填字段是否存在
- 字段类型是否正确
- 枚举值是否合法

## 9.2 节点合法性
- `entry` 指向的节点必须存在
- 节点 `id` 不能重复
- 节点 `id` 不应与内置类型保留字冲突（如 `exec`、`verify`）
- 节点 `type` 必须属于支持集合

## 9.3 边合法性
- `from` / `to` 必须指向存在的节点
- `on` 必须属于合法 outcome 集合
- `session` 必须属于 `continue | new`

## 9.4 `exec-plan` 合法性
- 每个 `exec` 节点若声明了 `planFrom`，该节点必须存在
- `planFrom` 指向的节点必须是 `worker`
- 且该 `worker` 必须显式声明 `primaryArtifact = "exec-plan"`

## 9.5 控制策略合法性
- `maxRepairLoops` / `maxAcceptanceLoops` 必须是正整数
- `onAcceptanceFailure` 必须属于 `auto_loop | stop`
- 若存在多个 `verify` 节点，应直接报错
- 若不存在 `verify` 节点却声明了 `onAcceptanceFailure`，应直接报错
- 不应再通过普通 edge 表达 `verify.failure -> worker` 的大循环回跳
- `maxRepairLoops` 的语义应按“repair 回跳次数”解释，而不是 attempt 总数
- `maxAcceptanceLoops` 的语义应按“新 acceptance round 创建次数”解释，而不是 round 总数

## 9.6 歧义提示
runtime 至少应给出 warning 或 error：
- 某个 `exec` 未声明 `planFrom`
- 某条边在当前策略下可能不可达
- 某个 loop 没有上限保护

补充说明：
- `worker` 未声明 `primaryArtifact` 本身不是 DSL 错误，也不应默认 warning
- 只有当某个下游节点或控制语义实际依赖该 `worker` 的 canonical artifact 时，才应要求显式声明对应 `primaryArtifact`

---

## 10. 一个最小示意

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
      "profile": "developer",
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

---

## 11. 与其他文档的关系
- [DSL 概览](overview.md)
- [worker 节点](nodes/worker.md)
- [exec 节点](nodes/exec.md)
- [verify 节点](nodes/verify.md)
- [Runtime Control](../runtime/control.md)

---

## 12. 一句话总结

> **Control DSL 负责显式表达 workflow 的控制面：节点怎么连、loop 怎么回、session 怎么处理、上限是多少，以及 runtime 启动前要检查哪些合法性。**
