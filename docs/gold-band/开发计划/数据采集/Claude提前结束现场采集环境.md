# Claude 提前结束现场采集环境

对应设计：`../../产品设计文档/provider/claude-incident-capture.md`。

## 判断与范围

此次是诊断采集补齐，不是业务 Bug 修复。既有本地实验已证明 Claude ACP 0.70.0 / CLI 2.1.232 可把缺少 terminal SSE 的响应映射为 `end_turn`；历史事故尚无独立网络证据，不能将该实验直接当作历史根因。

## 实施与验收

- [x] 保持原 adapter 版本，独立安装固定依赖，不编辑上游包。
- [x] 新建/恢复会话自动注入 raw SDK 和 debug 配置。
- [x] SDK 结束边界、result、后台生命周期与 ACP RPC ID 独立记录。
- [x] 分卷保留旧事件，容量/写入失败明确标记 incomplete。
- [x] 接口测试覆盖 metadata 保持、敏感正文剔除、分卷、容量耗尽、原样错误、cancel 与 end_turn 后续活动。
- [x] 真实 adapter + 本机模拟 API 验证正常结束及两类缺失 SSE 终止事件的异常，并确认 native debug 文件生成。
- [x] 官方 OpenTelemetry Collector 0.160.0 安装、SHA-256 校验、自动启动与真实 request ID 导出验证。
- [x] 验证后激活用户持久启动配置、保存回滚信息。
- [x] 用户重启后，phase-5 实际业务会话已确认使用采集入口并取得结束边界证据。

验证命令：

```powershell
node --test scripts/diagnostics/claude-capture/capture.test.mjs
node scripts/diagnostics/claude-capture/verify.mjs
```

`verify.mjs` 使用本机合成响应、虚拟凭据、禁用工具和独立配置目录，不调用真实模型。验证的是采集证据和转发不变性；对故障例仍保留原 adapter 的 `end_turn`，不宣称修复了上游缺陷。

## 本机部署

部署脚本 `install.mjs prepare` 将运行文件复制到用户 `.gold-band/diagnostics/claude-capture/`。完成依赖和 collector 安装后，在该目录运行测试及 `verify.mjs <该目录>/validation`；`enable` 要求真实验证报告通过后才修改 settings 中 Claude 的启动项与日志级别，并登记当前用户登录自启动。`disable` 从 activation-backup 恢复启动项并删除自启动登记，不删除已采集证据。

外部 HTTP/SSE 代理未部署，API 地址保持原配置。独立网络流或网关服务日志仍是进一步定位远端断流原因的额外证据。

2026-09-05 本机验收：四项接口测试通过；安装副本真实 adapter 三种响应验证通过，三段会话遥测全部落盘，存在 `req_capture_normal` 请求 ID。CLI 二进制与事故安装的 SHA-256 相同。collector 健康接口返回 200，工作集约 106 MiB。用户设置已改为包装器启动及 debug 日志，当前用户 Run 自启动项已登记。报告位于 `.gold-band/diagnostics/claude-capture/validation/verification.json`；未中断原业务进程。

## 验收评审

设计复杂度限定为外部诊断入口，不新增 Gold Band 状态机、数据库或完成判定分支。测试未引入真实模型费用；日志增长有明确边界，token 仅汇总计数。collector、native debug 与 adapter 自身的存储策略不同，不能统一承诺不会覆盖。当前任务不依赖 UI 变化，也不要求重新构建 EXE。

## 2026-09-06 升级复测

- [x] 按用户要求升级到当前 npm latest：claude-acp 0.75.1，官方配套 SDK 0.3.257 / CLI 2.1.257；未强行替换为独立最新 SDK。
- [x] 用户已停止客户端任务，直接替换原采集目录依赖；settings 命令、参数、API 配置和历史日志不变，安装前的 0.70.0 启动备份保留。
- [x] 仓库依赖和 lockfile 同步升级；激活检查使用声明的依赖版本；验证使用所选 adapter 的配套 CLI，并把实际入口记录到报告。
- [x] 四项接口测试通过；新版真实 adapter 的正常响应、thinking EOF、partial text EOF 三例完成，三个原生 debug 文件及三段会话遥测均验证成功，测试进程正常退出。
- [x] 升级报告保存在 `.gold-band/diagnostics/claude-capture/validation-0.75.1/verification.json`，旧报告保留；验证进程已退出。用户要求清理临时安装后，确认 `adapters/0.75.1` 未被配置或进程引用，使用非强制删除成功清理；正式安装与历史日志保留。
- [ ] 用户完全退出并重新打开客户端后，使用新版跑实际业务，观察是否复发。

两类模拟不完整响应各发起两次 API 请求，最终仍产生 SDK 成功结果（stop_reason=null）与 ACP `end_turn`。这确认该异常路径在当前官方适配器依赖组合中仍可复现，不等价于定位真实现场最初断流原因。此次是依赖升级与诊断回归，并未修改 Gold Band 完成判定，也不宣称修复上游 Bug。

验收评审：复用既有采集及测试机制，不新增状态、缓存、队列或常驻进程。运行期日志频率和有界存储不变；增加的依赖解析只发生于验证脚本启动，无业务热路径扫描。无需重新打包客户端。
