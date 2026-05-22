# Gold Band Provider 概览

## 1. 核心判断
Gold Band 以 provider 为核心抽象，当前默认 provider 已切换为 `claude-acp`：通过 ACP-compatible adapter 调用 agent，并使用 ACP 统一后的 session events 作为会话详情可视化输入。

Claude Code direct CLI / stream-json 不再作为新运行路径的 fallback；历史 run 中的 legacy 文件仅作为日志/诊断材料读取。

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

## 3. Provider 路线

### ACP-first provider
优先接入：
- `claude-agent-acp` / `claude-acp`
- `codex-acp`
- `gemini` ACP mode
- 其他 ACP-compatible agent adapter

Claude ACP 默认通过 `npx -y @agentclientprotocol/claude-agent-acp@latest` 启动；Windows 桌面运行时仅在进程启动边界把 bare `npx` 解析为 `npx.cmd`，其他平台不做命令改写。

### Legacy 历史数据
新运行不再启动 `claude-code` direct CLI / stream-json。若旧 run 已存在 `progress.events.jsonl` 或 `raw.stream.jsonl`，只能通过日志/诊断入口查看，不能形成第二套主会话 UI。

## 4. 后续可扩展 provider
- 支持 ACP 的 coding agent adapter
- 暂不支持 ACP 但可作为 debug fallback 的 CLI agent

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
- ACP session events 是 provider 返回值的统一观测输入，但不作为稳定控制流依据
- provider raw frame / raw stream 仅用于排障与 raw viewer，不作为 UI 主协议
- 不再新增 Gold Band 自研 `progress.events.jsonl` 作为 provider 输出统一层
- workflow / profile 的解析优先级应在 runtime 上层统一完成，而不是由 provider implementation 自行猜测

## 7. 一句话总结

> Provider 层的任务，是优先通过 ACP adapter 统一不同 agent 的会话返回值，并把 provider-specific SDK / CLI 差异隔离在 adapter 边界内；Gold Band runtime、artifact 和 workflow control 仍保持自己的 canonical state。
