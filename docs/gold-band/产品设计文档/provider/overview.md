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

用户开启“使用本地 Claude”时，桌面端只负责为 ACP adapter 注入 `CLAUDE_CODE_EXECUTABLE`，不改变 adapter 命令本身。Windows 下必须避免把 npm 暴露的 extensionless `claude` shell shim 传给 adapter：优先使用 PATH 中的原生 `claude.exe`；若 PATH 目录暴露了 `claude.cmd` 或 extensionless `claude`，则按标准 npm global prefix 结构查找同 prefix 下的 `node_modules/@anthropic-ai/claude-code/bin/claude.exe`；若无法定位包内原生 binary，则不注入该环境变量，让 adapter 使用自身 fallback。macOS / Linux 继续按 PATH 查找 `claude`。

### 项目级 app config
项目内需要版本控制的共享运行配置，统一放在仓库根目录 `configs/app-config.json`。

当前规则：
- `configs/app-config.json` 属于项目级配置，随仓库版本管理。
- 这类配置用于控制 runtime / provider / UI 的共享能力，不放入用户本机 `settings.json` 或 `state.json`。
- CLI 与桌面端都读取同一份 app config，并在运行时合并到 `RuntimeConfig`。
- 默认值以代码内 `RuntimeConfig::default()` 为准，`configs/app-config.json` 只覆盖明确声明的字段。

当前已落地的配置示例：
- `acpSessionTitleRefreshEnabled`：控制 ACP 会话运行期间是否定时调用 `session/list` best-effort 刷新并持久化 session title 缓存；默认关闭。
- `acpChatEventPageSize`：控制前端 ACP 会话历史分页的单次加载条数；默认 360。前端会额外保留有限多页内存缓冲以保证滚动连续性。

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
