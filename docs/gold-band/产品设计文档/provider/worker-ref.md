# Worker Ref 规范

## 1. 一句话定义
`worker-ref.json` 用来保存某个 attempt 对应的 **provider-specific worker 引用信息**。

它的作用不是替代 ACP session events / provider 原始 transcript，而是让 Gold Band 能够：
- 记录这次 attempt 实际调用了哪个 provider / ACP adapter
- 保存该 provider 返回的 ACP session id / 继续引用
- 在需要时继续或打开原始 worker 会话
- 将 provider-specific handoff 差异收敛到一个清晰边界文件里

## 2. 设计原则

### 2.1 `worker-ref.json` 是边界文件，不是业务产物
它不参与：
- workflow 成功判断
- `worker / output validation` 的控制流判断
- artifact 语义计算

### 2.2 由 runtime 落盘
provider adapter 可以返回 `worker-ref` 原材料，但 canonical 的 `worker-ref.json` 应由 runtime 写入 user project runtime store 中的 attempt 目录，不写入项目工作树。

### 2.3 provider-specific 细节只能暴露在这里
例如：
- ACP 的 `session_id`
- Claude Code legacy 的 `session_id`
- 某个 provider 的 `conversation_id`
- 某个 provider 的 continue token / session 引用
- 对应的打开/继续命令模板

## 3. 最小结构

```json
{
  "version": "0.1",
  "provider": "claude-acp",
  "mode": "new",
  "supportsOpenSession": true,
  "supportsContinueSession": true,
  "continueRef": {
    "acpSessionId": "4aefdd5f-1b5c-47d0-92a3-69005afb53f9",
    "adapterId": "claude-agent-acp",
    "adapterDisplayName": "Claude ACP",
    "cwd": "<workspace>",
    "sessionFile": "<attempt>/acp.session.json",
    "lastStopReason": "end_turn",
    "restored": false
  },
  "openCommand": null
}
```

## 4. 最小必填字段
- `version`
- `provider`
- `mode`
- `supportsOpenSession`
- `supportsContinueSession`

说明：
- `mode` 当前最小枚举：`new | continue`
- `continueRef` 允许 provider-specific 内部结构
- `openCommand` 允许为空
- `mode` / `continueRef` / `supportsContinueSession` 描述的是 provider 会话复用能力，不等同于 CLI 层的 `continue` / `retry` 动作

## 5. runtime 校验规则
以下情况应视为 `invalid`：
- 缺少 `version`
- 缺少 `provider`
- 缺少 `mode`
- `supportsOpenSession` 不是 boolean
- `supportsContinueSession` 不是 boolean
- `mode` 不在 `new | continue` 之内

以下情况不应直接视为错误：
- `continueRef = null`
- `openCommand = null`
- `supportsOpenSession = false`
- `supportsContinueSession = false`

## 6. 与 CLI / Console 的关系
`worker-ref.json` 直接支撑：

```bash
gold-band run open-session <run-id> --round round-001 --node develop --attempt attempt-002
gold-band run continue <run-id>
```

但两者语义不同：
- `open-session`：Gold Band 读取 `worker-ref.json` 后，把控制权交给 provider，按 provider-native open/continue 方式打开原始会话
- `run continue`：仍是 Gold Band runtime 控制动作；runtime 可读取 `worker-ref.json` 来触发 provider resume，但不会把控制流切换为 provider handoff

CLI / Console 的最小消费方式：
1. 用 `run-id + round-id + node-id + attempt-id` 唯一定位 attempt
2. 读取 `worker-ref.json`
3. 检查 `supportsOpenSession` / `supportsContinueSession`
4. 对 `open-session`：若 `supportsOpenSession = false`，CLI 必须明确报错；若 `openCommand.command` 存在，则优先使用它；否则交给 provider adapter 构建 provider-native 打开命令
5. 对 `run continue`：仅在 runtime 需要恢复 provider 会话时，才使用 `continueRef` / provider adapter 的继续能力
6. 对 console：应把 `worker-ref` 作为独立 detail tab 展示，而不是混入 config/workflow snapshot 视图

## 7. 与 layout 的关系
固定路径位于 user project runtime store：

```text
~/.gold-band/projects/{project-id}/tasks/<task-id>/runs/<run-id>/rounds/<round-id>/nodes/<node-id>/attempt-<n>/worker-ref.json
```

项目工作树中的 `<repo>/.gold-band` 只用于项目级配置覆盖，不存放 `worker-ref.json`。

## 8. 一句话总结

> `worker-ref.json` 是 Gold Band 在 user project runtime store 中保存 ACP / provider-specific 会话引用的统一边界文件；它不参与控制流判断，只负责让应用、CLI 或插件能够在需要时继续或打开原始 worker 会话。
