# Gold Band 产品概览

> GOLD BAND 西游记中的金箍，象征着对于能力强大但是无规章制度事物的约束 -- 适当地约束往往能让其能力变得更加强大
> 此处代表的是针对claude code、codex cli等code agent的约束，用程序化的手段，严格控制其工作状态流转与循环

## 一句话定位
Gold Band 是一个**以 provider 为核心抽象、优先通过 ACP 统一 agent 返回值**的轻量工作流 runtime。

它的分工是：

- 默认 authoring / deep-dive 工具负责需求澄清、需求文档生成与 workflow DSL 生成
- Gold Band runtime 负责执行、调度、校验、恢复与 canonical state
- ACP 负责统一 agent/provider 的会话返回值，Gold Band 基于 ACP session events 做会话详情可视化
- Gold Band 的核心 runtime / artifact / layout 模型保持 provider-agnostic

## 要解决的问题
Gold Band 主要解决 3 个问题：

1. 控制流依赖模型上下文，容易漂移
2. 子执行单元难以自然复用用户已有生态
3. “是否完成”不能只靠 agent 自报

## 核心原则

### 1. provider-first / ACP-first
- Gold Band 的核心抽象是 provider
- provider 输出统一优先交给 ACP，不再长期维护多套 provider-specific 可视化协议
- Claude Code direct 仅作为 legacy / fallback / debug 路径；后续 Claude Code 主路径应通过 `claude-agent-acp`
- 后续扩展优先选择 ACP-compatible provider adapter

### 2. runtime 控制流外置
- 控制面 deterministic
- 执行面 probabilistic
- 完成判断基于 artifact 与验证，而不是 self-report

### 3. desktop-first，但本质是 runtime-first、command-first
- 桌面客户端是当前主产品入口，面向本地项目的任务编排、执行观测与恢复操作
- Rust runtime / storage / DSL / provider adapter 是 canonical backend contract
- CLI 保留为脚本化、调试和自动化入口，但不再主导产品交互心智
- 新前端 / 插件层必须复用 runtime 契约，而不是基于日志或 provider 输出重新推断终局状态

### 4. step-first，而不是 chat-first
- Gold Band 桌面端不提供自然语言命令入口
- 核心操作通过菜单、按钮、表格、工作流画布、抽屉和设置页完成
- 若后续接入需求 / workflow 的 AI 生成能力，应放在 authoring 层扩展，而不是把运行主路径改造成 chat

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
3. 通过 ACP session events 在会话详情中查看原始 agent 过程
4. 在需要时通过 `worker-ref` 回到对应 provider CLI 深查或接管原始会话

## 当前文档分层
- 交互层：见 [交互层概览](../interaction/overview.md)
- Provider 层：见 [Provider 概览](../provider/overview.md)
- DSL：见 [DSL 概览](../dsl/overview.md)
- Runtime / Layout：见 [Runtime 概览](../runtime/overview.md)

## 当前仍待继续细化
- ACP provider capability matrix 的扩展项
- 节点状态文件 schema 细节
- ACP session events 到会话详情 ViewModel 的映射
- external CLI handoff 与 ACP session/load 的关系

## 需求标题归一化工具
- 当前仓库提供一个面向中文 requirement 文本的本地标题归一化实验工具：`src/bin/requirement_title.rs`
- 输入为 txt / md / 任意纯文本文件路径，输出一个约 10 字左右、可读的短标题
- 当前版本采用纯 Rust 的分层回退管线，不依赖大模型：优先抽取文档主标题等结构信号；缺少结构时回退到前导主题句；再不行时使用轻量统计压缩候选标题
- 当前实现尽量依赖通用特征，例如结构层级、位置、重复度、技术实体显著性和长度控制，而不是持续追加业务特殊词表
- 当前只优先支持中文需求；后续若扩展多语言，应在通用管线之上补 language profile，而不是重写整套流程

当前 MVP 已固定的 capability fallback 包括：
- 显式请求 `session = continue` 但 provider 不支持时，视为 DSL 校验错误
- `supportsOpenSession = false` 时，CLI `open-session` 明确报错
- ACP session events 不可用时，会话详情退化为 raw/debug 输出和外部 CLI handoff，不新增 provider-specific UI 协议
