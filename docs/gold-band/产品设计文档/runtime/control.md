# Runtime Control 规范

## 1. 定义
Runtime Control 是运行时状态机：它读取当前 worker 节点的 `NodeOutcome`，按 workflow edge 决定下一步，并负责 run / round / node 状态落盘。

## 2. 节点模型
当前 runtime 执行 `worker` 与 AI-DYNAMIC 派生节点。节点 outcome 只表达业务结果，不表达运行异常。节点 outcome 来自三种路径：

1. provider 成功且无需产物校验：`success`。
2. AI 输出验证：读取 `output.artifact`，按 `success_condition` 得到 `success / failure`；声明了 `output.schema` 且输出不合法时进入内部 `invalid` 修复流程。
3. 人工 check：会话结束后暂停，用户提交成功或失败。

provider/auth/quota/rate-limit/model/catalog/transport/IO 等异常必须先归一化为 `RuntimeErrorInfo`，再映射到 `runtime-abnormal` 或 `error-blocked`。它们不能写成 `NodeOutcome::Failure`，也不能驱动 `failure` edge。

模型配置过期不属于运行异常。模型目录是 provider 的快速变化能力事实：加载或保存 workflow template、创建或读取 task authoring workflow、以及创建或加载 run snapshot 时，若最新 Agent diagnostics 已提供非空模型目录且明确不再包含配置模型，runtime 将对应字段规范化为“不指定”、持久化回作者态 JSON 与运行快照，并在 run 生命周期记录结构化 `model_config_normalized` 事件；这样 UI、原始 JSON、快照与实际调用不会分叉。若目录缺失或为空，runtime 保留原值，不能把“无法确认”误判为“已经过期”。ACP `session/new/load` 返回权威 config options 后必须再次校验；若此时确认模型过期，则跳过模型配置 RPC、清除本次 override、记录 `acp_model_config_normalized` 诊断并继续使用 provider 默认模型。

## 3. 控制决策

| 当前 outcome | 决策 |
| --- | --- |
| `success` | 查找 `on=success` edge；无 edge 则等价于隐式 `success -> $end`，run success |
| `failure` | 查找 `on=failure` edge；无 edge 则等价于隐式 `failure -> $end`，run failure |
| `invalid` | 不查找 edge；若来自 `output.schema` 不合法则同 attempt 隐藏追问修复，最多 3 次；修复耗尽后 run failure |
| `killed` | run 完成 killed |
| `None` | run 暂停，保留当前节点与 attempt |

edge target 规则：

- 指向 worker：创建目标节点的新 attempt 并继续执行。
- 指向 `$end`：根据 edge outcome 完成 run。
- 指向 `$new-round`：关闭当前 round，创建新 round，并从 edge 的 `new_round_entry` 解析下一轮起点；`success -> $new-round` 在 DSL 校验阶段被拒绝。

`failure` edge 只承接业务失败：artifact 结构合法，但 success condition 明确判定不通过，或人工 check 明确判定失败。运行异常、provider 异常和 adapter/ACP 异常不属于 failure edge 输入。

## 4. session 继承
- `session=new`：目标 worker 新开会话。
- `session=continue`：仅当目标 provider 支持 continue session 时可用。
- continue ref 来自目标 worker 节点当前最新 attempt 的 worker ref；找不到时降级为普通新会话上下文。
- 上一节点的 primary/output artifact 可作为 feedback summary 进入下一次 worker 调用。

ACP invocation 的 continue prompt state 由 runtime 统一决策，普通 workflow worker 与 AI-DYNAMIC 内部 worker / acceptance / merge 复用同一套规则：

- 新 session 使用 `RequirementTask`。
- continue session 且用户没有显式输入时使用 `WorkflowResume`，发送 runtime 默认继续提示。
- continue session 且用户有显式输入时使用 `UserMessage`，只发送用户输入原文，不重新注入 hidden runtime context，也不包装 `# Goal` / `# 用户提示` / `# Task`。
- runtime repair 使用 `RuntimeRepair` 覆盖普通 continue 决策。

各节点类型只提供自己的 continue ref 来源；不得在普通 workflow、AI-DYNAMIC worker / acceptance / merge 中分别复制 prompt mode 判断。

## 5. attempt 限制
节点跳转不再使用 repair loop 概念，而由显式 edge 创建目标节点的新 attempt。例如：

```json
{ "from": "test", "to": "dev", "on": "failure", "session": "continue" }
```

`control.max_attempts` 表示当前 round 内的修复/重试预算，只统计由 `failure` 触发、且 edge 指向真实 worker 节点的修复跳转。正常 `success` 前进不消耗该预算；`output.schema` 不合法触发的隐藏追问不新增 attempt，也不消耗该预算。例如 `max_attempts = 1` 时，`test failure -> dev` 可修复一次，修复后的 `dev success -> test` 仍应继续执行。超过预算时 runtime 不再创建新的 attempt，当前 run / round 以 failure 结束，并写入结构化 `workflow_control_limit_exceeded` 事件用于 UI 展示停止原因。没有声明 `max_attempts` 时不限制。

## 6. 新 round
`$new-round` 用于表达验收类 worker 未通过后的下一轮执行：

```json
{ "from": "accept", "to": "$new-round", "on": "failure", "new_round_entry": "$entry" }
```

新 round 使用同一 workflow snapshot。`new_round_entry="$entry"` 表示从当前 workflow 的 `entry` 开始；也可以填写任一真实 worker 节点 id，让下一轮从该节点开始。历史 task / run 如果缺失 `new_round_entry`，运行启动、重跑冻结 snapshot、以及运行态读取 frozen snapshot 时，都会在 snapshot 校验前仅对 `$new-round` 边补为 `$entry`，让旧数据继续按当时的“从 workflow entry 重开”语义执行；规范化结果只写入本次 run 的 `workflow.snapshot.json`，不回写 `authoring/workflow.json`，作者态新保存的 workflow 仍然必须显式声明该字段。下一轮的 hidden runtime context 不会直接继承上一轮完整前序链；只有当前 round 的入口节点会额外看到入口之前的稳定前缀节点最新产物，以及触发 `$new-round` 的上一 round 最后节点原因。例如 `A -> B -> C` 且新 round 从 `B` 开始时，入口 `B` 会看到 `A` 的产物和上一轮 `C` 触发重开的原因；本轮 `B` 重新执行后，后续 `C` 只看到本轮 `B`，不继续携带上一轮 `A/C` 的附件或触发原因。触发 `$new-round` 的上一 round 最后节点会作为“进入本轮的原因”写入入口节点 hidden context 的前序流转原因，包含该节点 output artifact、预览和 attachments，但不进入 predecessor chain。若 workflow 声明了 `control.max_rounds`，该值限制 `$new-round` 可打开的新 round 数，初始 round 不计入；超过限制时当前 run / round 以 failure 结束。

## 7. 人工 check 暂停
启用 `manual_check=true` 的 worker 在 provider 会话自然结束后进入：

- run: `paused`
- round: `paused`
- node: `paused`
- pause reason: `waiting-for-user-input`

人工 check 暂停不是 runtime continue：当前 ACP 会话的输入区保持可用，用户可以继续发送普通 ACP prompt 追问或补充上下文，这些消息不会触发 workflow edge。会话面板额外展示“成功 / 失败”判定按钮；只有用户点击其中一个按钮后，runtime 才写回 `NodeOutcome` 并继续按 edge 流转。

`manual_check_pending` 必须持久化在当前 attempt 的 `node.json` 中。应用关闭后再次打开，只要 run / round / node 仍处于上述暂停态且 `manual_check_pending=true`，会话面板仍应恢复判定按钮和可用输入区，点击成功或失败后继续推进 runtime。

## 8. 可恢复运行异常
以下情况进入 `paused + runtime-abnormal`：

- 本地 IO、系统资源或临时文件写入异常，例如 Windows `os error 1450`。
- ACP transport 断开、adapter stdout 断开、driver 线程提前退出等会话仍可能继续的运行期异常。
- auth、quota、rate limit、provider 暂不可用、model invalid、catalog missing、provider 缺失、workspace 能力缺失等用户处理外部条件后可继续的异常。
- 事件、timeline、raw frame 等观察性写入失败，且不会改变 workflow 前提条件。

`runtime-abnormal` 与用户停止的 `process-interrupted` 都保留当前 run / round / node / attempt，并允许通过 runtime continue 恢复；区别是前者需要以异常视觉提醒用户排查本地、协议层或 provider/config 条件。用户点击继续且后端接受后，会话输入区立即进入 runtime 控制态并保持锁定，直到 runtime active、停止中、错误或下一次可交互暂停事实到达；后台写入 running 文件前残留的旧 paused / interrupted-input 快照不得让输入区短暂恢复为可输入。错误分类优先使用 runtime 内部 typed error 与 source chain 中的 `std::io::Error` / transport error；只有 adapter、ACP 或第三方库没有稳定错误类型时，才允许在统一 normalization 层用字符串特征作为最后兜底。

### 8.1 用户停止优先级与 ACP 收尾

用户停止是 attempt 生命周期中的持续事实，不是一次性的 `session/cancel` 边缘通知。Stop 在本地 attempt 创建后即可接受，不依赖 provider 先报告 active：prompt 尚未发出时直接终止 dispatch；prompt 已发出但 provider 尚未 active 时发送一次 cancel，并在后续观察到 `threadStatus=active` 且 prompt 尚未 terminal 时补发一次。两次投递分别通过 `before-provider-active / after-provider-active` 门闩去重，同一阶段不得重复发送。

runtime 写入 `Paused + ProcessInterrupted` 后，自动重试控制器必须在错误分类前、backoff 期间、runtime 重建前和再次发送 prompt 前重新读取当前 attempt 事实；只要 attempt 已停止，就不得写入新的 `runtime_auto_retry` 或再次调用 provider。停止后的 provider 输出不再进入当前 turn；晚到 provider/transport 错误只进入取消收尾诊断，不能覆盖用户停止终态，也不能重新触发自动重试。

`provider.server-unavailable` 等 `RecoveryMode::Auto` 错误使用共享的 `RetryPolicy`，默认在初次调用后最多自动重试 3 次。AI-DYNAMIC 自动重试必须保持原 attempt、logical prompt 与 session mode，不生成 proposal repair prompt；预算耗尽后才收敛为 `Paused + RuntimeAbnormal`。运行时自动恢复与输出协议 repair 是两套独立状态机，调用次数和验收必须从共享 retry policy 推导，不能在测试或实现中另行硬编码。

`session/cancel` 后仍需有界等待原 `session/prompt` terminal。若 deadline 到期，记录结构化 `acp.cancel-drain-timeout`，用户可见 attempt 仍保持 `Paused + ProcessInterrupted`，ACP turn 结算为 cancelled；该未收尾 session 必须从 attempt route、attached runtime registry 和 worker continue ref 中移除，后续继续使用新 session。adapter process 仍按 `provider_id + workspace_root` 复用，不因单个 session 收尾超时被 kill，也不得影响同 process 上的其他 session。

若 ACP 在 session-ready、session id 或首批 timeline event 形成前已经进入 `runtime-error`，会话 UI 必须优先展示 runtime diagnostic 错误态并停止初始 loading；不能因为 session snapshot 尚未 ready 而持续显示加载中。已经建立 session 或已有事件的会话仍走正常会话错误展示路径，避免初始化错误规则覆盖可恢复的既有会话。

## 9. 错误阻塞
以下情况进入 `paused + error-blocked`：

- workflow / DSL 无效或 workflow snapshot 与 runtime 状态不一致。
- AI-DYNAMIC proposal repair 耗尽后仍不合法。
- AI 输出验证声明了产物但产物缺失，且 repair 机制耗尽。
- dynamic 控制约束或 runtime invariant 被破坏，无法确定安全恢复点。

`error-blocked` 表示当前 runtime 路径不可直接恢复；UI 可以展示错误详情和按错误类型派生的处理入口，但不能把它当成普通 runtime continue 输入。处理入口不等于继续，只有后端验证存在安全恢复点并生成明确恢复计划时，才允许恢复；否则只能重新运行、从节点重新开始或进入诊断流程。

## 10. 状态一致性
每次节点进入、完成、暂停、跳转或打开新 round 时，runtime 必须同步更新：

- `run.json`
- `round.json`
- `node.json`
- round trace
- progress snapshot / run events

runtime 落盘完成后，前端可见状态必须继续通过 lifecycle/run-state 事件刷新：`RunCompleted` 和 `RunPaused` 都需要发出 run-state 更新事件，前端收到后重新拉取后端 Conversation VM。人工 check 的 `waiting-for-user-input`、运行异常、用户停止等暂停态都不应由前端本地猜测或按 `manual_check_pending` 打补丁修正；前端只消费刷新后的后端 lifecycle/composer 事实。

## 11. 控制 JSON 展示标注

当普通 worker 的 output contract 或 AI-DYNAMIC 的 `dynamic-node-completion` 被 runtime 作为控制输入提取后，runtime 会对当前 attempt 的 ACP timeline 写入展示标注：

- 标注位置：对应 assistant `textDelta` item 的 `raw.runtimeControlOutputDisplay`。
- 标注内容：`artifactName`、`kind`、`jsonText`、`start/end`、`jsonStart/jsonEnd`、`fenced`、`parseStatus`。
- `parseStatus` 可以是 `valid` 或 `invalid`。runtime 控制和 artifact 解析仍只接受合法 JSON；`invalid` 只表示该 assistant 输出中存在 JSON-like 控制候选，且本轮将进入 repair 或阻塞处理。
- 同一条 assistant 输出中同时存在合法完整 JSON 与更靠后的非法 JSON-like 嵌套片段时，展示标注必须优先选择合法完整 JSON；非法 span 只作为没有合法 JSON 时的 fallback。
- `start/end` 使用前端 JavaScript 字符串可直接消费的 UTF-16 索引，用于展示层把自然语言和控制 JSON 拆分。
- 前端展示为单行折叠控制条：收起态不展示 JSON 内容，展开后才展示完整格式化 JSON；`valid` 使用主色和控制清单图标，`invalid` 使用告警色和告警图标。
- 该标注只服务 UI 展示，不参与 artifact 内容、schema 校验、success condition、edge control 或 repair 判断。
- 标注失败不得阻断 runtime 主控制流；artifact 提取、落盘和校验仍以 Rust 端既有 JSON 扫描与 validation 为准。
