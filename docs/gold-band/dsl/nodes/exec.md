# `exec` 节点规范

## 1. 当前定位
`exec` 节点严格消费结构化执行计划，并按 group 与依赖关系执行命令。

## 2. 当前已知结论
- 不解释自然语言
- 不自由决定跑什么命令
- 主要消费 `exec-plan.json`
- 主要产出 `exec-result.json`
- group 内顺序执行，group 间按依赖关系调度

## 3. 当前关注点
- 当前 round 内 `exec-plan` 的解引用方式
- group 依赖与失败处理
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

## 5. 相关文档
- [DSL 概览](../overview.md)
- [exec-plan](../artifacts/exec-plan.md)
- [exec-result](../artifacts/exec-result.md)
