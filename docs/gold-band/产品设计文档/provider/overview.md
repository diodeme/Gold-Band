# Gold Band Provider 概览

## 1. 核心判断
Gold Band 以 provider 为核心抽象，默认提供 Claude Code provider 实现。

## 2. provider 层职责
provider adapter 负责：
- 启动 provider worker
- 传入 prompt / input
- 接收最终结果
- 返回 worker reference 原材料
- 提供会话继续/打开能力
- 暴露 provider 能力信息

Gold Band 核心 runtime 不应直接了解：
- 某个 provider 的 stdout 格式细节
- 某个 provider 的 session 继续参数细节
- 某个 provider 的内部 transcript 布局

## 3. 当前默认 provider
- `claude-code`

## 4. 后续可扩展 provider
- `codex`
- `gemini-cli`
- `open-code`
- 其他 AI worker / CLI agent 工具

## 5. 当前子文档
- [Provider Adapter 接口](adapter.md)
- [Worker Invocation Contract](invocation.md)
- [Prompt Bundle 规范](prompt-bundle.md)
- [Worker Ref 规范](worker-ref.md)
- [Claude Code Provider 实现](implementations/claude-code.md)

## 6. 当前约束
- 核心模型 provider-first
- 默认实现可以写 Claude Code，但不得把 Claude-specific 细节写死为唯一语义
- canonical artifact contract 必须保持 provider-agnostic
- provider-specific 引用只能通过 `worker-ref` 等边界文件暴露
- provider 的 raw stream 只能作为观测增强，不作为稳定控制流依据
- workflow / profile 的解析优先级应在 runtime 上层统一完成，而不是由 provider implementation 自行猜测

## 7. 一句话总结

> Provider 层的任务，是把不同 AI worker / CLI agent 工具的差异隔离在 adapter 边界内，让 Gold Band 的 runtime、artifact、layout 和 interaction 层保持 provider-agnostic。
