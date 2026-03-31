# Gold Band Runtime 概览

## 1. 核心对象
Runtime 当前围绕以下对象组织：
- task
- run
- round
- attempt
- node
- artifact
- worker reference

## 2. 目录层级模型
当前推荐 4 层：

```text
preset -> task -> run -> round/attempt
```

## 3. 当前关键结论
- 顶层对象不是 conversation，而是 task
- session 复用不等于产物目录复用
- runtime 只信规范化产物，不信模型自己起的文件名
- 节点之间通过 runtime registry 传递产物，而不是直接猜路径
- `status` 与 `outcome` 必须分离：`status` 表示生命周期，`outcome` 表示终局结果
- `paused` 只属于 `status`，不属于 `outcome`

## 4. 子文档结构
- [控制层](control.md)
- [目录布局](layout.md)
- 状态文件规范
  - [task.json](state/task.json.md)
  - [run.json](state/run.json.md)
  - [round.json](state/round.json.md)
  - [node.json](state/node.json.md)

实现时建议先看：
1. [控制层](control.md) —— 状态机、continue/retry/kill、transition table
2. [run.json](state/run.json.md) —— run 级生命周期与终局状态
3. [round.json](state/round.json.md) —— round 级循环与挂起状态
4. [node.json](state/node.json.md) —— attempt 级状态与 outcome

## 5. 解析优先级

### workflow 解析优先级
建议统一为：
1. CLI 覆盖参数 `--workflow`
2. task 目录下的默认 workflow
3. 项目目录下的预设 workflow
4. 用户目录下的预设 workflow

### provider 解析优先级
建议统一为：
1. 当前节点显式声明的 `provider`
2. runtime 内部默认 provider（当前 MVP 为 `claude-code`）

### profile 解析优先级
建议统一为：
1. 项目目录下的 profile
2. 用户目录下的 profile

约束：
- 若 `worker` / `verify` 节点声明了 `profile`，runtime 在 `run start` 时必须解析成功，否则直接失败
- `validate_workflow()` 只负责结构校验；profile 是否存在属于 runtime resolution

## 6. 状态语义总说明
MVP 中建议统一遵循：

- `status`：生命周期状态，使用 `running | paused | completed`
- `outcome`：终局结果
  - `node`：`success | failure | invalid | killed | null`
  - `run / round`：`success | failure | killed | null`

统一约束：
- `status != completed` 时，`outcome = null`
- `status = completed` 时，`outcome` 必须为终局值
- `paused` 只表示 runtime 观测到的系统挂起态，不表示终局结果
- `failure` 表示目标未达成或执行失败
- `invalid` 表示结果不满足最小 contract

## 7. runtime 配置
当前 runtime 相关配置统一由 `RuntimeConfig` 管理，至少包括：
- `default_provider`
- `log_level`
- `log_prompts`
- `log_provider_command`
- `log_retention_days`

边界：
- 配置由 CLI 或上层入口构造
- `App` 持有配置并向 observability / provider 执行链透传
- provider command 与完整 prompt 仅属于 debug observability，不属于 canonical state

## 8. 与 console / 插件的关系
- console CLI 是同一套 runtime 的交互壳，不引入新的 runtime 语义
- scriptable CLI、console CLI、VSCode 插件都应复用同一套 run / round / node / attempt / artifact 模型
- UI 可以提供更强的浏览、帮助与下钻体验，但不能改变 canonical state 的来源与控制流语义

## 9. 相关边界文件
- [Worker Ref 规范](../provider/worker-ref.md)
- [Progress 规范](../interaction/progress.md)
