# Claude 提前结束现场采集

## 目的与边界

针对业务尚未完成却收到 ACP `end_turn` 的现场，保留 SDK result、流结束标记、后台任务事件、ACP 请求与响应的关联证据。现有实验可以复现“不完整 SSE 被 SDK/adapter 返回成功”，但不能直接认定历史事故来自同一根因。本次只补充诊断，不修改业务完成判定。

原有设计由 ACP 统一 provider 接入，Gold Band 管理工作流状态。诊断使用外部 stdio 包装器及 adapter 已有 `_meta.claudeCode.emitRawSDKMessages`、SDK debug 和 OpenTelemetry 功能；不修改 claude-acp 源码，不引入 provider 私有业务状态机。包装器源码位于 `scripts/diagnostics/claude-capture/`，运行副本位于用户目录 `.gold-band/diagnostics/claude-capture/`。

## 数据与生命周期

- 每次 adapter 进程创建独立采集目录，包含 PID、父 PID、工作目录、版本、UTC 毫秒时间、单调时间和递增序号。
- 新建、load、resume、fork 注入原始 SDK 事件过滤器和独立 native debug 文件。保留其他 metadata、模型、权限、工具和设置选项。
- ACP 请求 ID、session ID、SDK message UUID、模型请求 ID 和 task ID 沿用来源身份。Prompt 仅记录长度和 SHA-256，用于与业务/finalize 请求对应，不复制完整提示词。
- `message_start`、`message_delta`、`message_stop`、result、session state、command lifecycle、task bookend 和后台任务集合进入独立 ledger。逐 token delta 仅统计数量、字节和最后事件时间。
- SDK 私有诊断通知由包装器消费；只有客户端原先明确订阅的通知才继续转发。标准 ACP 帧与停止原因不改写。
- 采集持续到连接/进程结束，不在收到 `end_turn` 时关闭。权限请求、cancel、错误、后续自主活动继续记录。
- 诊断不是 canonical state，任何采集状态都不得推进或延迟业务 finalize。

## 保存与故障

- 生命周期 ledger 每 8 MiB 分卷，单连接上限 512 MiB，不删除既有分卷。stderr 每 8 MiB 分卷，上限 64 MiB。
- 达到上限或写入失败时，输出 `CLAUDE_CAPTURE_INCOMPLETE` 并尽可能写入 incomplete 文件，继续协议转发。异常终止后 manifest 停留在 recording，不能视作完整关闭。
- native debug 与 adapter 日志使用每连接独立目录；当前不自动清理，需留意磁盘总量。它们可能包含敏感内容，只保留本机。
- OpenTelemetry 使用官方 collector，仅监听 loopback。遥测文件每 16 MiB 轮转，保留 64 份备份、14 天，属于独立的有限保留策略。完整 API 请求/响应正文默认关闭。
- collector 登录时隐藏启动，包装器启动前检查并确保服务就绪；每分钟健康检查，故障记录缺口并尝试重新启动，恢复不能抹掉已有缺口。已有业务进程不被中断；保存启动配置后需重启 Gold Band 才能保证采用新入口。
- 此安装不改变 API endpoint，不部署 HTTP 中间代理。SDK 流事件不能冒充独立 HTTP/SSE 证据；如需判断网关为何断流，仍需独立网络采集或按 request ID 调取网关日志。

## 依赖升级

2026-09-06 经用户要求，将采集环境从 claude-acp 0.70.0 / SDK 0.3.232 升至当日最新版 claude-acp 0.75.1，沿用上游固定的 SDK 0.3.257 / CLI 2.1.257，不单独覆盖 SDK 版本。用户已停止任务，直接升级原目录依赖；settings 中的包装器入口、采集目录、保留策略和旧日志保持原状，客户端重启后采用新版。

验证入口默认复用采集配置的实际 adapter 路径，从该安装解析配套原生 CLI；可显式指定待验证入口。每次升级使用独立验证目录，保留旧报告；manifest 和报告中的版本记录实际加载值，不反写历史证据。安装激活校验从依赖声明读取版本，避免写死初次事故版本。

0.75.1 的真实适配器验证确认正常响应、两类缺失结束事件的响应及遥测均可采集；异常例仍返回 `end_turn`，因此升级不构成提前结束问题已修复的证据。

## 自评审

- 过度设计：复用原 adapter、SDK、标准 OpenTelemetry Collector 和成熟 JSONL 分帧库，仅增加本机诊断包装器；不修改产品状态模型。
- 性能：单次经过 ACP JSONL，token 内容不持续落盘；两个方向的写入都遵守背压，诊断磁盘写异步且每个消费循环等待完成。每 60 秒一个 heartbeat；无历史扫描、无全量会话正文复制。collector 使用内存限制与有界 batch。
- 风险：开启 SDK 原始流会增加 adapter 到包装器的序列化量；它们不进入 Gold Band UI。无法承诺无限日志保留、强制断电无尾部损失或第三方服务内部可观测性。
