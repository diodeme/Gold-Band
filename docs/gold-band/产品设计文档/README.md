# Gold Band 文档导航

Gold Band 当前文档按目录式结构整理为 5 个主板块：

## 1. 产品设计
- [产品概览](product/overview.md)

## 2. 交互层
- [交互层概览](interaction/overview.md)
- [CLI 规范](interaction/cli.md)
- [Console 概览](interaction/console-overview.md)
- [Console 信息架构](interaction/console-information-architecture.md)
- [Console 命令模型](interaction/console-command-model.md)
- [Console 状态与事件](interaction/console-state-and-events.md)
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
- 输出与结果判定
  - worker 节点通过 `output` 声明输出 DSL，通过 `success_condition` 判断 success / failure；schema 输出不合法时自动隐藏追问修复
  - 人工 check 通过 `manual_check` 声明，且与 AI 输出验证互斥

## 5. Runtime / Layout
- [Runtime 概览](runtime/overview.md)
- [控制层](runtime/control.md)
- [目录布局](runtime/layout.md)
- 状态文件规范
  - [task.json](runtime/state/task.json.md)
  - [run.json](runtime/state/run.json.md)
  - [round.json](runtime/state/round.json.md)
  - [node.json](runtime/state/node.json.md)

## 当前原则
- 文档主内容统一维护在 `docs/gold-band/` 下
- 已定内容继续沉到对应子文档，未定内容先在对应子文档中占位
- 当前桌面端工程统一使用 `npm` 作为仓库级包管理器，依赖锁文件以根目录 `package-lock.json` 为准；未完成明确迁移前，不引入第二套仓库级 lockfile
