# Gold Band Agent 会话观测规范

## 1. 一句话定义

Gold Band 后续不再把 provider 输出蒸馏成自研 `progress.events.jsonl`。

新的观测目标是：**通过 ACP 调用 agent/provider，保留 ACP 统一后的 session events，并在 Gold Band 的会话详情中可视化展示原始 agent 过程。**

## 2. 核心决策

旧设计：

```text
provider raw stream
  -> Gold Band progress.events.jsonl
  -> CLI / 插件 / UI 进度展示
```

新设计：

```text
ACP-compatible provider
  -> ACP session events
  -> Gold Band 会话详情 ViewModel
  -> UI 可视化 / raw 查看 / 外部 CLI handoff
```

因此：

- `progress.events.jsonl` 不再作为新增目标。
- provider 可视化优先基于 ACP `session/update`、tool call、plan、permission、error 等统一返回值。
- Gold Band 不再为 Claude Code stream-json、Codex JSON、Gemini stream 等分别维护长期 UI 协议。

## 3. 观测层文件边界

### 3.1 ACP raw

建议 attempt 级保留：

```text
acp.raw.jsonl
```

用途：

- 保存 ACP stdio 原始 frame 或 adapter 原始事实。
- 仅用于排障和 raw viewer。
- UI 默认不直接依赖其字段做业务判断。

### 3.2 ACP timeline

建议 attempt 级保留：

```text
acp.timeline.jsonl
```

用途：

- 保存 ACP 会话聚合后的 UI timeline final item。
- 保持 ACP 语义，但粒度是可直接渲染的逻辑 item，而不是逐 chunk 原始事件或中间修订 patch。
- 作为会话详情 UI 的主要数据来源。

可包含：

- user message
- assistant message
- thought / reasoning block
- tool call 及其原地更新
- plan
- permission request / decision
- mode / config / model update 投影
- session info
- status update
- error / stop reason

### 3.3 ACP session metadata

建议 attempt 级保留：

```text
acp.snapshot.json
```

用途：

- adapter binary / package 信息
- ACP protocol version
- session id
- capabilities
- model / mode / config 摘要
- stop reason
- startedAt / finishedAt
- external CLI handoff 信息摘要
- timeline / usage / metrics 的当前恢复锚点

### 3.4 ACP diagnostics

attempt 级额外保留：

```text
acp.diagnostics.jsonl
```

用途：
- adapter stderr / 启动失败 / initialize、load、prompt 错误。
- 未识别 ACP frame 或无法归一化的事件。
- UI session header 中的 error count / last error 摘要。

### 3.5 run-progress.json

`run-progress.json` 可以继续作为 run 级快速快照，表达“workflow 当前走到哪里”。

但它不再承载 provider 过程细节，也不替代 ACP 会话事件。

## 4. 与 canonical state 的关系

ACP 事件属于观测与会话可视化，不直接决定 runtime 控制流。

以下仍是权威状态：

```text
run.json
round.json
node.json
artifact files
worker-ref.json
```

ACP 事件不能直接决定：

- workflow 是否成功
- node outcome
- run outcome
- edge routing
- artifact 是否有效

这些仍由 Gold Band runtime、control engine 和 artifact validator 决定。

## 5. 会话详情读取优先级

当 CLI / 桌面端需要展示某个 attempt 的 agent 过程时：

1. 首选 `acp.snapshot.json` + `acp.timeline.jsonl` 构建会话详情。
2. 若需要排障，展示 `acp.raw.jsonl`。
3. 会话详情 composer 的状态以用户可感知阶段为准：调起 ACP 到用户消息写入前是发送中且不计时；用户消息写入到首个非用户帧前是处理中并计时；首帧后按思考、工具调用或回复生成继续计时；plan 决策权限若提供继续规划选项，composer 保持可输入但停止处理中计时，并用输入框 placeholder 引导用户自然语言修订计划。
4. 若 agent/provider 不支持 ACP，显示 fallback / debug 输出，但不新增 provider-specific UI 协议。
5. `worker-ref.json` 提供打开外部 CLI / 原始 agent 会话的 handoff。
6. `run.json` / `round.json` / `node.json` 用于确认 canonical 状态。

## 6. 原 progress.events.jsonl 的废弃说明

`progress.events.jsonl` 的原意是 Gold Band 自己定义一套 provider-agnostic 事件流。

该方向废弃，原因是 ACP 已承担统一 agent 返回值的职责。如果继续保留自研 progress event，会导致：

- ACP 与 Gold Band progress 两套协议并存。
- Claude Code direct 与 ACP provider 输出语义不统一。
- UI 需要维护两套可视化适配。
- ACP 的 tool call / plan / permission / terminal 等原始上下文被蒸馏丢失。

后续如遇旧数据中的 `progress.events.jsonl`，可作为 legacy/debug 内容读取，但不作为新功能设计入口。

## 7. 外部 CLI handoff

Gold Band 会话详情必须保留“打开原始 agent 会话 / 跳转外部 CLI”的能力。

原则：

- Gold Band 内部负责可视化 ACP session events。
- 用户仍可跳到原始 provider CLI 继续处理。
- handoff 信息通过 `worker-ref.json` 或 `acp.snapshot.json` 暴露。
- handoff 不改变 Gold Band runtime 的 run / round / node canonical state。

## 8. 一句话总结

> Gold Band 的观测层从自研 progress events 转向 ACP session events：ACP 统一 agent 返回值，Gold Band 基于 ACP 会话做可视化，同时继续保留自己的 runtime canonical state 与外部 CLI handoff。