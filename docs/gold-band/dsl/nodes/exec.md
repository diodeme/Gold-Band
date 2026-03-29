# `exec` 节点规范

## 1. 当前定位
`exec` 节点严格消费结构化执行计划，并按计划中的命令顺序串行执行。

## 2. 当前已知结论
- 不解释自然语言
- 不自由决定跑什么命令
- 主要消费 `exec-plan.json`
- 主要产出 `exec-result.json`
- 不允许并行
- 命令按计划顺序执行

## 3. 当前关注点
- 当前 round 内 `exec-plan` 的解引用方式
- 串行执行时的失败与 skipped 规则
- `cwd` 缺省规则
- `exec-result` 的最小字段与判断规则

## 4. `planFrom` 的解引用规则
当前建议：
- `planFrom` 必须指向某个 `worker` 节点
- runtime 只在当前 round 内解析 `planFrom`
- 若同一 round 内该 `worker` 有多次 attempt，则取**最新一次 attempt** 的 `exec-plan`
- 若当前 round 内该 `worker` 还没有产出合法 `exec-plan`，则当前 `exec` 节点不应启动，应视为控制层或前置状态错误

说明：
- `exec` 不回看上一轮 round 的产物
- 小循环中的 plan 更新，天然通过“当前 round 最新 attempt”收敛

## 5. 执行规则
- `exec` 只执行 `exec-plan` 中显式给出的命令
- 命令按数组顺序串行执行
- 不做 group 调度，不做依赖调度
- `cwd` 未声明时，默认使用 workspace root
- `timeoutSec` 若存在，则按该命令自己的超时上限执行
- 不做 shell / 平台差异标准化，模型给出的命令原样交给执行层
- 每条已处理命令的执行状态应直接汇总进 `exec-result.json.commands[]`
- `commands/` 目录中的 sidecar 只保留命令定义与日志，不再要求单独的 `result.json`

## 6. 失败与 skipped
- 某条命令执行失败后，`exec` 的整体结果由 `exec-result.status` 决定
- 首版建议串行执行采用 fail-fast：当前命令失败后，不再继续执行后续命令
- 未执行到的后续命令在 `exec-result` 中记为 `skipped`
- `skipped` 只表示“按当前串行执行规则未执行”，不表示命令本身成功或失败

## 7. 相关文档
- [DSL 概览](../overview.md)
- [exec-plan](../artifacts/exec-plan.md)
- [exec-result](../artifacts/exec-result.md)