# Claude 本机诊断采集

此目录是独立采集工具，不修改 Gold Band 的 finalize 判定或 claude-acp 源码。当前依赖固定为 claude-acp 0.75.1 / SDK 0.3.257 / CLI 2.1.257，沿用适配器官方依赖组合。初次事故采集使用 0.70.0 / SDK 0.3.232，历史证据中的版本不作改写。

安装位置：`%USERPROFILE%\.gold-band\diagnostics\claude-capture`。

## 使用

激活配置后，在当前任务结束时完全退出并重新打开 Gold Band。此后新建/恢复会话自动注入采集参数，无需每次开终端。旧进程不能补录过去的事件。

OpenTelemetry Collector 随当前用户登录隐藏启动，包装器还会在启动时检查并拉起它。采集只写本机，未启用 HTTP 中间代理或完整 API 正文。

## 文件

- `captures/<UTC时间-PID>/events-*.jsonl`：SDK 流边界、result、后台任务、ACP 请求/响应和心跳。每 8 MiB 分卷，上限 512 MiB，不删除旧分卷。
- 同目录 `native-*.debug.log`：每次会话创建/恢复的 Claude 原生 debug。
- 同目录 `agent.log`：适配器自身日志。
- 同目录 `stderr-*.jsonl`：stderr，单连接上限 64 MiB。
- 同目录 `manifest.json`：版本、路径、进程关联和采集结束状态。`recording` 只表示未记录到正常关闭，不能证明进程仍活着。
- 同目录 `*.incomplete.json`：容量/写入失败标记。收集器中断也记录为缺口。
- `telemetry/events*.jsonl`：原生请求/错误遥测及请求 ID。16 MiB 轮转，保留 64 份、14 天。
- `collector-service/`：收集器自身启动日志。
- `validation/verification.json`：本地真实 adapter 联调报告。
- `validation-0.75.1/verification.json`：2026-09-06 升级后的独立验证报告，包含实际适配器和 CLI 路径。
- `activation-backup.json`：原始 Claude 启动设置和日志级别。

原生 debug 与 adapter 日志没有额外自动清理；留意磁盘空间。文件可能含工具输出或其他敏感内容，不要直接公开整包日志。

复发时保留整个相关连接目录，以及对应 Gold Band attempt 的 `acp.diagnostics.jsonl`、`acp.raw.jsonl` 和原生 transcript。SDK 层缺少 message_stop 不等价于已经证明网络层缺失 message_stop；若需追查网关内部原因，还需独立 HTTP 证据或服务商按 request ID 查询日志。

## 恢复

停止后续采集可执行安装目录下的 `node install.mjs disable`，它恢复原启动项并删除登录自启动登记；重启 Gold Band 后生效，不删除证据，也不主动停止可能仍服务现有会话的 collector。

原始 `activation-backup.json` 仍指向启用采集前的 0.70.0；恢复原配置同时会恢复该旧版本。当前升级直接替换采集目录依赖，未修改用户 settings 中的包装器命令和参数；完全退出并重新打开 Gold Band 后采用新版。

## 验证

`node --test capture.test.mjs` 验证协议转发、权限、取消、后续活动、分卷和容量标记。`node verify.mjs <独立验证目录> [adapter入口]` 使用虚拟凭据和本地模拟接口验证真实 adapter，不调用远程模型。默认复用采集配置的 adapter 入口，并从其依赖中解析配套 CLI，避免验证与实际运行使用不同安装。

0.75.1 验证中正常响应、两类不完整响应的 SDK/ACP 事件、三个原生 debug 文件和遥测均成功采集；两类不完整响应仍返回 `end_turn`。验证通过表示采集正常，不表示上游提前结束问题已经修复。
