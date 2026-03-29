# Gold Band 文档导航

Gold Band 当前文档按目录式结构整理为 5 个主板块：

## 1. 产品设计
- [产品概览](product/overview.md)

## 2. 交互层
- [交互层概览](interaction/overview.md)
- [CLI 规范](interaction/cli.md)
- [Progress 规范](interaction/progress.md)

## 3. Provider 层
- [Provider 概览](provider/overview.md)
- [Provider Adapter 接口](provider/adapter.md)
- [Worker Invocation Contract](provider/invocation.md)
- [Prompt Bundle 规范](provider/prompt-bundle.md)
- [Worker Ref 规范](provider/worker-ref.md)
- [Claude Code Provider 实现](provider/implementations/claude-code.md)

## 4. DSL
- [DSL 概览](dsl/overview.md)
- [Control DSL](dsl/control.md)
- 节点规范
  - [worker 节点](dsl/nodes/worker.md)
  - [exec 节点](dsl/nodes/exec.md)
  - [verify 节点](dsl/nodes/verify.md)
- 标准产物规范
  - [exec-plan](dsl/artifacts/exec-plan.md)
  - [exec-result](dsl/artifacts/exec-result.md)
  - [verify-result](dsl/artifacts/verify-result.md)

## 5. Runtime / Layout
- [Runtime 概览](runtime/overview.md)
- [控制层](runtime/control.md)
- [目录布局](runtime/layout.md)
- 状态文件规范
  - [task.json](runtime/state/task.json.md)
  - [run.json](runtime/state/run.json.md)
  - [round.json](runtime/state/round.json.md)
  - [node.json](runtime/state/node.json.md)
  - [manifest.json](runtime/state/manifest.json.md)

## 当前原则
- 文档主内容统一维护在 `docs/gold-band/` 下
- 已定内容继续沉到对应子文档，未定内容先在对应子文档中占位
