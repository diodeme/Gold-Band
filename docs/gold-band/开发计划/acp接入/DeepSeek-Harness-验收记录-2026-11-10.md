# DeepSeek Harness 真实验收记录（2026-11-10）

## 结论

Windows x64 上，固定 npm 基线 `@deepseek-ai/dsh@0.2.1-rc.2` 的 DeepSeek Harness ACP 真实验收通过。本次同时记录了原生 EXE `0.2.2-rc.1` 的附加验证结果；该版本不是本方案的 Catalog `validatedVersion`，不能替代固定基线。

本次验收只覆盖 Windows x64。macOS、Linux 和 Windows aarch64 仍未测试；Qoder 尚未按方案验收。因此 DeepSeek Harness 本身达到当前平台的真实验收证据要求，但第一阶段 Catalog 仍不得扩展为 13 项。

## 被测对象

| 目标 | 版本 | 运行时 | 供应链 | 结果 |
| --- | --- | --- | --- | --- |
| npm 基线 | `@deepseek-ai/dsh@0.2.1-rc.2` | Node `v22.22.3` / npm `12.2.1` | npm registry 固定版本 | ACP 生命周期与 Skill 通过 |
| 原生 EXE | 安装包 `0.2.2-rc.1`，产品版本 `0.2.2.0` | 捆绑 Node `v24.21.0` | 官方固定下载，Authenticode Valid | 附加 ACP 生命周期与 Skill 通过 |

原生安装包来自 `https://download.deepseek.com/desktop/dsh-latest-windows-x64.exe`，大小 `293425336` 字节，SHA256：

`830947F8996DC05D991CA9133FFF4D3400073E47B9CFA6507754867E79482B23`

签名主体为 `Hangzhou DeepSeek Artificial Intelligence Co., Ltd.`。该版本与固定 npm 基线不同，只作为桌面宿主附加证据。

## ACP 能力

两个版本在真实 `initialize` 中返回一致的关键能力：

- `agentInfo.name = deepseek-harness-acp`。
- `promptCapabilities.image/audio/embeddedContext = false`。
- `mcpCapabilities.http = true`。
- `sessionCapabilities.close/list/resume = {}`。
- 未声明 `loadSession`。

因此恢复路径只使用 `session/resume`；本次未调用 `session/load`。

## 生命周期验收

使用隔离临时 cwd 驱动 `dsh --profile acp`，执行：

1. `initialize` 成功。
2. `session/new` 返回 `sessionId` 和 `configOptions`。
3. 真实 prompt 触发 `agent_thought_chunk`、`tool_call`、`tool_call_update`、`agent_message_chunk` 和 `usage_update`；只读工具完成后返回 `TOOL_OK:D5H2`。
4. 第二个 prompt 在 `pwsh` 工具处于 `in_progress` 时发送 `session/cancel`；工具更新为 `tool call aborted`，`session/prompt` 返回 `stopReason = cancelled`。
5. 关闭第一条 transport 后重新 `initialize`，对新连接执行 `session/resume`；继续 prompt 正确回忆 `D5H2`。
6. `session/close` 返回成功；进程未在 15 秒内自行退出，随后按有界清理策略强制结束，未残留 ACP 子进程。

### 基线 0.2.1-rc.2 结果

- Session ID：`e3deca69-25d0-4da5-aef2-45758092fd48`。
- 真实工具：`pwsh` 完成，`tool_call` / `tool_call_update` 均可观测。
- Stop：活动工具被取消，`stopReason = cancelled`。
- Resume：新 transport 成功，最终输出 `D5H2`。
- Skill：见下节。

### 原生 EXE 0.2.2-rc.1 附加结果

- Session ID：`0aa4e11b-569e-42a8-8c98-699787aded1b`。
- 真实工具：`pwsh` 完成，thought/tool/final/usage 均可观测。
- Stop：活动工具被取消，`stopReason = cancelled`。
- Resume：新 transport 成功，最终输出 `D5H2`。
- Skill：见下节。

## Skill 验收

在隔离 cwd 创建项目级探针，并通过真实 ACP prompt 让模型调用 `skill` 工具：

- `<cwd>/.dsh/skills/accept-dsh/SKILL.md`。
- `<cwd>/.agents/skills/accept-agents/SKILL.md`。

两个版本均实际调用 `accept-dsh` 和 `accept-agents`，最终返回 `SKILL_BOTH_OK`。

为覆盖全局读取路径，临时创建 `~/.dsh/skills/accept-global-dsh/SKILL.md`。固定 npm 基线在同一个真实回合中依次调用项目 `.dsh`、项目 `.agents` 和用户 `.dsh` 三个 Skill。验收后该临时全局探针已从用户目录移出，原路径已确认不存在。

## 证据与复现

验收驱动为仓库内 `scripts/deepseek-acp-acceptance.mjs`。它通过 newline-delimited JSON-RPC 驱动 `dsh --profile acp`，自动响应一次性工具授权，输出事件摘要；不读取、输出或持久化凭据。

固定基线复现命令：

```powershell
node scripts/deepseek-acp-acceptance.mjs `
  "$env:USERPROFILE\AppData\Roaming\fnm\aliases\default\node.exe" `
  "$env:USERPROFILE\npm\node_modules\@deepseek-ai\dsh\lib\bin.js" `
  --profile acp
```

原始报告位于本轮临时目录，SHA256 分别为：

| 报告 | SHA256 |
| --- | --- |
| 原生 EXE ACP 生命周期 | `061AF6FBEC2FB0A4A72FDCF4BA28EBFFC1F90A36518FF3248075F332B11C8738` |
| npm 基线 ACP 生命周期 | `CA2E465E0912B7AC9FC8B3CCB13047E926F3ED7E778D46420BD6189EE34C48C5` |
| npm 基线项目 Skill | `52CA35FEE0950E888F946309C6BD584E85B380764D9DA5D0B5D80F98D9EB7AA0` |
| 原生 EXE 项目 Skill | `1E371743975CE10CB8E46EAF2C864B63AF9FCB3DF35E8853916F08BFBEE836ED` |
| npm 基线全局 + 项目 Skill | `468D5739266E237368D2649F869CD5CC43A1FF55E10B5B188EABB69CC80B81C6` |

## 发布边界

2026-11-12 更新：Qoder 已通过 Windows x64 真实验收，DeepSeek Harness 已写入正式 local supplement（固定 `0.2.1-rc.2`），正式 Catalog 扩展为 13 项。其余平台仍按发布矩阵逐项实测，未测试平台不得标支持。
