# Claude 后台禁用与结束意图确认

关联问题：https://github.com/diodeme/Gold-Band/issues/114

## 根因与范围

2026-09-06 的现场包含两种不同问题：等待后台任务时正常结束的 prompt 被立即要求收尾；未闭合的工具参数流被 SDK 报为 success/tool_use，ACP 转成 end_turn。后一现场中，随后发送的 finalize 被后台通知触发的续做处理，但结果仍归属 task-notification，客户端请求未返回。流式中断的更底层原因未知，本次不声称修复 SDK。

临时措施在 ACP 边界统一禁用 Claude 后台执行，并把 finalize 提示改为结束意图确认。确认部分优先只修改中英文 prompt，保留原有调用、Artifact 校验、repair、停止和预算机制；不引入等待标签、计时、轮询或凭 JSON 强制终止。

## 实施与验证

- [x] 发布并回读核对 issue #114。
- [x] 建立接口失败测试：真实子进程收到环境值 0，session/new 缺少 Monitor 禁用项；其他 provider 保持不变。
- [x] 注入启动环境和 session/new/load/resume/fork 的 SDK 选项，保持用户保存配置不变。
- [x] 提示词接口测试在旧版本失败，证明旧提示明确禁止业务继续；修改双语 finalize 提示词，保留输出协议及作用域边界。
- [x] 运行相关接口/提示词/生命周期回归，并在隔离 Claude ACP 中验证工具禁用与同步命令。
- [x] 同步设计文档并记录最终结果和剩余限制。

## 验证记录（2026-09-07）

- 修复前：`acp_claude_execution_policy` 的启动环境与 session 参数两项失败，分别得到 `0` 和缺失 `Monitor` 的禁用列表；另一 provider 不受影响的反例通过。`artifact_finalize_confirms_intent_and_allows_business_continuation` 在旧提示上失败，输出仍含“不要继续执行任务”。
- 修复后：94 项定向测试通过，包括 `provider::tests` 43 项、`acp::adapter::tests` 11 项，以及 `acp_claude_execution_policy` 4 项、`acp_connection_logging` 1 项、`provider_output_priority` 3 项、`provider_prompt_bundle` 31 项、`runtime_control_output_provenance` 1 项。假适配器子进程与真实安装探针在普通测试中保持 ignored，由对应入口显式运行。
- `scripts/diagnostics/verify-claude-background-policy.mjs <acp_claude_execution_policy 测试程序> <已安装 Claude ACP dist/index.js>` 通过 Rust 的实际 AdapterConnection 启动入口运行 ACP 0.75.1 / SDK 0.3.257 / CLI 2.1.257；模型响应来自本地 HTTP fixture，不使用线上模型或真实凭据。
- 实测工具 schema 不再包含 Bash/Agent 的 `run_in_background`，且不包含 Monitor；强行调用后台 Bash 与 Monitor 都得到工具错误。12 秒 Bash 正常同步返回，general-purpose 子 agent 同步返回最终结果，四次 prompt 均返回 end_turn。项目 `.claude/settings.json` 明确写入禁用值 `0` 的反例仍被 Runtime 的 `1` 覆盖。
- 首次同步子 agent 探针因 fixture 将父工具输入中的子提示误判为子请求而缺少验收观测；修正为按最新 user 文本区分后通过。这是测试路由修正，不是生产异常。
- `rustfmt --check`、验证脚本语法检查与 `git diff --check` 通过。构建存在原有 orchestrator dead-code warnings，本次未改动相关代码。
- 前端已在独立端口 1431 启动；内置浏览器报告 `No browser is available`，按技能诊断确认列表为空，未能完成可视验收。本次无前端代码或 UI 提示变更。验证用前端、适配器、HTTP 服务和临时 session/workspace 已清理。
- 本次未重新打包或替换已安装 EXE，需使用包含这些源码改动的新客户端构建后生效。

## 剩余边界

AI-DYNAMIC 的 PostTurn 确认调用正常结束但没有 Artifact 时，重新确认结束意图；有无效候选输出时才修复协议。确认与修复共享原有最多 3 次后续请求上限，达到上限仍按原错误边界处理，不会无限询问。本次不调整普通 Workflow 的输出重试分支或 InlineControl 语义。策略不是 sandbox，也不会停止已存在的后台工作；未完整流被 SDK 误报成功的根因仍在 issue #114 中保留为未知。

## 缺失 Artifact 的后续确认（2026-09-07）

- 根因：AI-DYNAMIC 原实现把缺失 Artifact 与提交无效 Artifact 合并为 repair，导致第二次成功返回但只含普通文本时重新禁止业务执行；属于结束意图确认设计落实不完整。现场 finalize 期间发生压缩，但压缩是否造成协议遗漏尚未确定。
- 修改：PostTurn 正常结束、无 Artifact 文件且无控制输出候选时，复用双语 finalize 模板及完整 schema，保留同一 session、节点和工作区；使用新的 prompt identity 和 FinalizingArtifact 阶段。有候选输出不合法时保留 repair，停止和失败不进入该分支。
- 最小失败证据：`dynamic_post_turn_missing_artifact_reconfirms_after_each_normal_end` 在旧实现失败，实际 RuntimeRepair，预期 RuntimeFinalize；修改分支后同一测试通过。
- 验证：47 项定向测试通过（dynamic_post_turn 3 项、InlineControl repair 1 项、provider::tests 43 项），覆盖连续缺失、完整 schema、隐藏提示、不同请求 identity、无效 Artifact 保留 repair，以及 provider 的停止和失败分类。旧 InlineControl 测试实际用 depth=1 构造了 PostTurn 节点，现改为 depth=0 并显式断言 emission mode 后通过。rustfmt 与改动文件的 git diff --check 通过；仅有原有 3 项 dead-code 警告，未打包或替换 EXE。
- 过度设计与性能评审：复用现有循环、模板、阶段和请求上限，不新增持久状态、依赖或后台任务；只检查已知 Artifact 路径，不扫描会话历史。额外模型调用受原有后续请求上限约束。本次无 UI 变更。

## 设计与性能评审

沿用 canonical provider ID claude-acp，不通过显示名或命令猜测类型。策略在 adapter 层统一应用，无新持久字段、依赖、状态机或 UI；创建与恢复 session 时只合并少量选项，工具去重为 O(n)，不扫描文件或历史、不增加后台轮询。现有会话和后台任务不被强行中断；新进程/新建或恢复的 SDK session 应用策略。
