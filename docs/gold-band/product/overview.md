# Gold Band 产品概览

> GOLD BAND 西游记中的金箍，象征着对于能力强大但是无规章制度事物的约束 -- 适当地约束往往能让其能力变得更加强大
> 此处代表的是针对claude code、codex cli等code agent的约束，用程序化的手段，严格控制其工作状态流转与循环

## 一句话定位
Gold Band 是一个**以 provider 为核心抽象、默认提供 Claude Code provider 实现**的轻量工作流 runtime。

它的分工是：

- 默认 authoring / deep-dive 工具负责需求澄清、需求文档生成与 workflow DSL 生成
- Gold Band runtime 负责执行、调度、校验、恢复与观测
- Gold Band 的核心 runtime / artifact / layout 模型保持 provider-agnostic

## 要解决的问题
Gold Band 主要解决 3 个问题：

1. 控制流依赖模型上下文，容易漂移
2. 子执行单元难以自然复用用户已有生态
3. “是否完成”不能只靠 agent 自报

## 核心原则

### 1. provider-first
- Gold Band 的核心抽象是 provider
- Claude Code 是当前默认 provider 实现
- 后续应可扩展到其他 provider

### 2. runtime 控制流外置
- 控制面 deterministic
- 执行面 probabilistic
- 完成判断基于 artifact 与验证，而不是 self-report

### 3. CLI-first
- CLI 是核心 backend 接口
- VSCode 插件主要封装 CLI
- 插件提供更好的可视化与控制体验

### 4. step-first，而不是 chat-first
Gold Band 的核心对象是：

- workflow
- node
- attempt
- artifact
- verifier
- continue / retry

## 当前主流程
1. 在默认 authoring 工具中生成 requirement / workflow DSL
2. 在 Gold Band 中执行 workflow
3. 在需要时通过 `worker-ref` 回到对应 provider 工具深查原始会话

## 当前文档分层
- 交互层：见 [交互层概览](../interaction/overview.md)
- Provider 层：见 [Provider 概览](../provider/overview.md)
- DSL：见 [DSL 概览](../dsl/overview.md)
- Runtime / Layout：见 [Runtime 概览](../runtime/overview.md)

## 当前仍待继续细化
- provider capability matrix
- 节点状态文件 schema 细节
- progress.events.jsonl 的精细事件模型
- stream 到 progress 的具体映射策略
