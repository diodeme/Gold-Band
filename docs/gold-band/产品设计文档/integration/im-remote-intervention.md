# 客户端直连 IM 的远程干预设计

## 1. 文档状态

- 状态：已批准，实施中
- 首期仅支持企业微信；微信延期，等待稳定主动推送真实 PoC
- 2026-09-03 范围收缩：删除第二平台 connector、SDK、通道枚举、设置接口和 UI，不保留隐藏入口或兼容消费路径
- 2026-09-04 可靠性闭环：补齐发送租约恢复、保留清理调度、generation-owned connector sender、删除清理 journal、局部重配置失败隔离、前端 occurrence identity 与投影队列背压契约
- 运行形态：Gold Band 桌面客户端进程内直连，不部署 Gold Band 云端 IM 网关
- 关联开发计划：[客户端直连 IM 远程干预实施计划](../../开发计划/IM接入/客户端直连IM远程干预实施计划.md)

## 2. 一句话定义

Gold Band 在桌面客户端内连接用户自己的企业微信机器人，把已有工作流干预和用户启用的 Run、ACP turn 信息通知投递到安装级全局一对一私聊；可操作请求经身份、状态与幂等校验后提交回现有 Runtime，信息通知始终只读。

## 3. 问题与设计结论

当前 Runtime 已经拥有权限请求、用户追问和人工结果判定的权威状态与提交接口，缺口是桌面端之外没有可达的通知和操作通道。因此问题不来自 Runtime 的根本设计缺陷，不需要重做工作流状态机；需要补齐的是统一的远程干预应用服务、平台连接器、可靠投递和安全凭据边界。

设计结论：

1. `RuntimeLifecycleBus` 是待干预事件的来源，Runtime 持久状态始终是唯一事实源。
2. IM 只是输入输出通道，不保存或推导第二套审批状态。
3. Tauri UI 与 IM 共用同一个 `InterventionCommandService`，不得互相调用外层 command。
4. 连接器只处理平台协议，平台 SDK DTO 不进入领域层。
5. 首期只使用平台交互卡片的明确控件提交动作（button 或 vote），不做自然语言 ChatOps。

ACP 权限请求的 canonical request identity 是接收 JSON-RPC 请求时固化到 pending state 和 typed `raw.requestId` 的原始 ID；该 ID 只用于 provider response 关联与 pending/response 文件查找。权限发生的 timeline/lifecycle identity 由 Gold Band 从 `requestId + provider toolCallId` 生成有界 `permission-<hash>`，provider 缺失 toolCallId 时使用已持久化的 Gold Band sequence，并写入 timeline raw 的 `_goldBandPermissionItemId`；同一 replay 保持相同，provider 重启后复用 requestId 的不同工具调用保持不同。前端 timeline reducer 必须以 `attempt/session scope + event kind + AcpUiEvent.id` 作为 occurrence key；同一 occurrence 的 pending/terminal snapshot 合并，复用同一 `raw.requestId` 的不同 occurrence 不得合并。用户追问继续使用 Gold Band 创建并同时写入 pending state 与 `AcpUiEvent.id` 的 `elicitationId`。lifecycle bridge 只读取这些结构化 identity，不做前缀裁剪、消息解析或 event ID 反查。

现有 `src/app/notification.rs` 的 `InterventionNotification` 和 `NotificationDedup` 继续服务于系统原生通知。它们表达的是“一次提醒、关闭后允许再提醒、点击后导航”，不具备远程动作所需的 request、revision、allowed actions 和持久幂等语义，因此不能充当 IM outbox 或入站动作账本。两者可以共享 lifecycle event identity 和 intervention kind 映射，但必须保持各自的投递生命周期。

```text
Runtime canonical state
        |
        | RuntimeLifecycleEvent::InterventionRequested
        v
RemoteInterventionService -----> bounded outbox in core.db
        |                                  |
        v                                  v
ImConnectionManager ------ platform connector ------ IM platform
        ^                                  |
        |                                  | callback / message
        +--------- RemoteInterventionAction+
                           |
                           v
              InterventionCommandService
                 |          |          |
       permission reply  elicitation  manual check
```

## 4. 范围

### 4.1 首期范围

- 在设置页配置、授权、启停和查看企业微信连接状态。
- 工作流进入权限审批、用户追问、人工检查等待态时发送通知。
- 在 IM 中同意或拒绝权限请求。
- 在 IM 中回答用户追问。
- 仅当当前 attempt 处于 `manual_check_pending` 时判定成功或失败。
- 展示客户端在线、授权待处理、退避重连、断开等连接状态。
- 支持离线期间的有界可靠投递、去重、过期与审计。
- 按逐类开关投递 Run 和 ACP turn 信息通知。
- 企业微信使用交互卡片；绑定目标只允许授权用户与机器人的一对一私聊。

### 4.2 非目标

- 不提供 Gold Band 托管网关、跨设备消息中继或云端凭据托管。
- 不把 IM 会话当作完整的工作流控制台或聊天历史镜像。
- 不支持任意自然语言解析、模型参与的指令判断或模糊匹配。
- 不支持通过 IM 启动、停止、重跑、编辑工作流或执行任意命令。
- 不支持群聊通知、群聊按钮或群聊文本指令。
- 微信在真实 PoC 证明稳定主动推送前不提供 connector、设置入口或 pull-only 降级。
- 不保证客户端退出、电脑休眠或断网期间仍能实时收发消息。

## 5. 平台能力与产品降级

| 能力 | 企业微信 | 微信 |
|---|---|---|
| 官方客户端直连方式 | 智能机器人 WebSocket | iLink HTTP 长轮询 |
| 主动发送 | 支持，但会话需先与机器人交互 | 稳定边界未经真实 PoC 证明 |
| 交互卡片 button / vote | 支持 | 不支持 |
| 更新原消息/卡片 | 支持 | 未验收 |
| 文本回答 | 支持 | 协议存在但不进入首期 |
| 单连接约束 | 单机器人只允许一个有效连接 | 未进入首期 |
| 首期定位 | 正式平台 | 延期 |

平台能力必须由连接器在运行时返回 `ImChannelCapabilities`。上层按能力投影交互，不允许按平台名称散落条件判断。

企业微信必须具备交互卡片能力。能力不足时该目标不生成可操作 delivery，不通过短码、自然语言或 pull-only 模式降级。

## 6. 用户流程

### 6.1 企业微信

1. 用户在设置页点击“扫码接入”，客户端从企业微信官方扫码授权端点取得短期二维码；用户使用手机企业微信扫码并确认授权。
2. Rust 后端轮询授权结果，取得 Bot ID 和 Secret 后直接写入操作系统凭据库；前端只接收二维码 URL、到期时间和脱敏状态，不接收 Secret。
3. 客户端连接 `wss://openws.work.weixin.qq.com`，连接成功后展示“在线”。
4. 用户先在目标会话中与机器人交互，Gold Band 记录允许主动投递的会话绑定；完成前设置页持续显示“等待绑定”并提示用户发送一条私聊消息，不把 WebSocket 在线误报为可投递。企业微信单聊事件没有 `chatid` 时，以 `from.userid` 作为 conversation/destination identity；不得因此退回群聊绑定。
5. Runtime 发布待干预事件，客户端发送模板卡片；卡片包含任务、节点、请求类型、到期时间和允许动作。企微 Permission、ManualCheck 与支持的 Elicitation 先发送一条 markdown 详情消息，再发送交互卡；两条标题携带同一 4 位 `display_ref`，详情 ACK 前不得发送卡片，卡片 subtitle 明确指向上一条详情。
6. 用户提交卡片动作后，连接器从官方回调结构中取得 `body.msgid`、`headers.req_id`、typed `event.template_card_event.event_key/task_id`；Permission vote 卡还必须取得唯一 `selected_items.selected_item[].option_ids.option_id[]`。Runtime 提交在 blocking pool 内完成，并须在平台 5 秒窗口内返回卡片更新。
7. 领域提交成功后使用同一回调的 `req_id` 与原卡 delivery identity 更新卡片为最终结果；显式返回的 `event.template_card_event.task_id` 必须等于该 delivery identity。button 回调省略可选 task 时可从 `event_key` 恢复原卡 identity；vote 回调的 task 为必填契约。若请求已被桌面端或另一消息处理，则显示“已处理”。发送 ACK 是否返回平台消息 ID 不影响这条回调更新路径。
8. IM outbox 固化的 `expected_state` 只包含决策语义：完整 locator、canonical request、原始 params/request schema、创建时间与完整允许动作。`timelineIdentity` 属于 Runtime 展示投影元数据，可在 outbox snapshot 之后绑定，不参与 CAS 指纹；ManualCheck 详情中的模型输出同样是投影快照，不参与 CAS 指纹。企微三类干预终态都必须保持原 `card_type`、原 `task_id`、原 `submit_button.key`，显式禁用协议支持禁用的 `checkbox` / `select_list` 并标记最终选择；官方 `submit_button` 类型没有 `disable` 字段，不得发送无效字段，也不得把可交互卡降级为只有标题的 `text_notice`。平台仍允许再次提交时，由入站 canonical event 幂等保证不重复执行 Runtime。

企业微信扫码协议复用官方 CLI 的 `/ai/qc/generate` 与 `/ai/qc/query_result`，3 秒轮询、5 分钟到期；安装级 `source` 位于 `configs/app-config.toml`，当前明确为 `maling`。跨企业授权能力仍须使用真实企业账号完成验收，不得仅根据可生成二维码宣称验收通过。企业微信心跳默认 30 秒；卡片回调的协议响应目标小于 5 秒，领域处理不得阻塞回调确认。同一机器人被其他客户端的新长连接替换时进入稳定连接冲突错误，不持续抢占重连。

主动推送严格对齐官方 `@wecom/aibot-node-sdk` 的 `aibot_send_msg` 帧：body 只包含 `chatid + typed message payload`，不得附加未定义的 `chat_type`。WebSocket 帧分发以是否携带 `cmd` 为第一契约：带 `cmd` 的帧是事件或业务请求，即使 `headers.req_id` 与 pending request 相同也不得消费回执；只有无 `cmd` 的帧才允许按 `req_id` 匹配 ACK。投递成功以存在且为数字的回执顶层 `errcode == 0` 为准，缺失或错误层级不得默认成功；官方回执未承诺 `msgid`，因此 ACK 可将 delivery 标记为 sent，但只有平台真实返回消息 ID 时才写 `im_delivery_bindings`，禁止构造伪平台消息 ID。未知平台错误只在脱敏诊断中保留数字码，对客和 SQLite outbox 仍只保存稳定错误码。

企业微信模板卡片不得把 Markdown 文档塞入 `main_title.title`。标题、描述、横向字段和控件文案分别按官方 26/30、5/26、10/11 字建议上限有界渲染；官方协议最多支持 6 个横向字段。Permission 与 ManualCheck 使用 `vote_interaction mode=0`；Permission 不超过 20 个选项，ManualCheck 固定为安全失败优先的两个结果。支持的 Elicitation 单题单选使用 `vote_interaction mode=0`，单题多选使用 `vote_interaction mode=1`，2-3 个单选题使用 `multiple_interaction`。`task_id` 使用 delivery 已有的 deterministic identity，满足同机器人唯一、允许字符和 128 字节上限，不新增卡片业务 identity；button `key` 只允许 `delivery_id:action_index`，vote/multiple 的 `option.id` 只允许本地短 index，submit key 只允许 `<delivery_id>:submit`，均不得携带 action JSON、签名 token 或其它安全状态。`sub_title_text` 的 112 字是官方建议而非协议硬上限；追问详情必须完整展示 typed 问题和固定选项，markdown 与卡片 payload 超 32 KiB 时只允许在构造层截断展示字符串，禁止截断序列化 JSON 或任何 identity 字段。

为消除 Permission 卡按钮横排截断，生产 presentation adapter 仅把 Permission 从 `button_interaction` 改为官方 `vote_interaction`：选项来自当前 pending permission 投影后的 allowed actions，`checkbox.mode=0`，`option.id` 只使用本地 action index 短引用，提交按钮使用 `<delivery_id>:submit`，`task_id` 继续复用 deterministic delivery identity。中文卡壳固定为 `权限审批（id=xxxx）/ Agent 请求执行命令`；`xxxx` 是从 durable outbox rowid 派生并补零到 4 位的 `display_ref`，取值为 `rowid % 10000`。它只用于用户把 markdown 详情与 vote 卡配对，不新增卡片业务 identity，不写入 expected state，也不得被回调解析或反查。同一 delivery 的发送顺序固定为“markdown 详情帧 -> 等待 ACK -> vote 卡帧 -> 等待卡片 ACK”；重试与重启后同一 rowid 保持同一编号，最近 10,000 次 outbox 插入内不会重复；只有卡片 ACK 成功才把 delivery 标记为 sent，详情帧没有独立 outbox 行或业务 identity。硬性协议闸门已经用真实企微账号完成：手机端选择 `option.id="0"`、PC 端选择 `option.id="1"`，真实回调均在嵌套 `template_card_event` 中返回 `selected_items.selected_item[].option_ids.option_id[]`，并能唯一还原出站选项；更新卡省略 `submit_button` 会触发平台码 42049，因此终态必须保留原 submit key。回调若只返回 submit key、缺少选择、多选、question key 不匹配或 task 不一致，均不得用默认选项、点击顺序或展示文案猜测授权语义。

Elicitation 使用 channel-agnostic 的 `RemoteElicitationForm` 契约，而不是把平台 DTO 提升为领域状态：`SingleScalarChoice` 表达一个 scalar 字段和固定选项；`MultiScalarChoice` 表达一个 array-of-scalar 字段、固定选项和是否允许显式空选择；`ScalarChoiceQuestions` 表达 2-3 个单选字段、selector key、field name、required 状态和固定选项。该契约挂在 typed allowed action 并参与 expected state；`timelineIdentity` 不参与指纹。企微只负责把该契约渲染为 vote/multiple 控件，并把回调转换成泛型 Form selection；selector key 固定 `q0/q1/q2`，option id 只使用本地短 index。回调必须校验 msgid、私聊 actor/conversation、delivery/task、submit key、question key、option id、required selector、重复选项和非法 index，再由 outbox 中的完整 typed scalar value 还原 content 并用原始 `requestedSchema` 编译校验。多选空数组只有在原始 schema 允许且回调显式返回空选择时可提交。`vote mode=1` 与 `multiple_interaction` 的真实回调形状尚未完成手机端/PC 端采样，接口 fixture 不是真实平台验收。

Elicitation markdown 详情除当前 message、任务/节点和题目选项外，还携带当前 root 分支最新可见 `textDelta` 的“前序模型输出”快照，用于解释“是否确认上述方案/分支”这类依赖前文的问题。读取必须复用 timeline index 并按 4096 字符有界，不读取完整历史、hidden thought、嵌套 Agent 分支或用户私密输入；缺失时不猜测。若前序输出、message 或 question description 规范化后相同，只渲染一次。

权限卡片的结构化事实只能从当前 `PendingPermissionState.params` 投影：`rawInput.command + args` 合成完整 `permissionCommand`，存在命令时不再把 command/args 降级为通用参数；其余请求保留 `permissionTool`、`permissionPath`、`permissionParameter` 与既有任务/节点字段，不再把 `permissionTitle` 降级为横向字段。`rawInput.cwd` 在没有 locations/path 时投影为执行路径。企微详情消息完整展示说明、工具、命令、路径、参数与任务/节点字段，正文和字段仍按 256 字符有界；vote 卡横向字段按“工具、命令、路径、参数、其他结构化字段”输出且遵守平台 26 字建议，超长事实由上一条详情承担。原始 `_meta.permission.title/description` 只留在 Runtime 提示语义中，不控制企微 Permission 卡壳。

ManualCheck 详情先读取当前 attempt ACP timeline 的最新可见 root `textDelta`，并按 4096 字符有界快照作为 `InterventionPrompt.message`；读取走既有 timeline index，只按排序候选读取命中的事件，不加载完整历史，也不把嵌套 Agent 分支、thought、空 chunk 或 hidden 输出当作当前节点结果。若暂停等待人工检查时缺失可见模型输出，投影返回稳定状态错误，不用上一 attempt、artifact 或相邻消息猜测。企微详情消息标题为“人工检查（id=xxxx）”，正文展示任务、节点和“最后一轮模型输出”；vote 卡使用同一标题并按“成功、失败”展示，成功默认选中。提交仍通过原 allowed action index 调用 `submit_manual_check`，不得用展示文案、展示位置或默认选项推断结果。

桌面按钮与 IM 提交 ManualCheck 时必须收敛到同一条桌面应用执行边界：先按 outbox/桌面 snapshot 校验 expected state 并获取 `ManualCheckSubmissionLease`，再执行 scheduled attention resume，把 resumed occurrence 写入本次 App context，配置与桌面续跑一致的 ACP live/session/prompt lifecycle callbacks，最后调用 `submit_manual_check_background` 并等待 Runtime launch ack。IM inbound 不得直接调用 Tauri command、绕过 scheduled resume 或启动缺少前端回调的 headless 续跑；两条入口只能有一个 first writer，重复提交仍由 lease 与 inbound 幂等收敛。

桌面按钮与 IM 提交 Permission 也必须收敛到同一条应用执行边界：expected state 校验与权限响应写入使用共享 `InterventionCommandService::execute`；随后统一执行 scheduled attention resume、记录 PermissionResolved metrics resume cause、重建 ACP session/dynamic session 投影、向前端 emit session update 并触发 attempt 索引。IM 不得只写权限响应而不清理前端 pending permission 投影；重复合法回调返回 `AlreadyApplied` 时仍要发出最新 session 投影。

桌面按钮与 IM 提交 Elicitation 也必须收敛到同一条应用执行边界：先 inspect 并校验 expected state，在 response signal 可见前 reclaim scheduled interaction，记录 ElicitationResolved metrics resume cause，再通过共享 `InterventionCommandService::execute` 写 response signal，随后重建 root/dynamic ACP session 投影、emit session update 清理桌面 pendingElicitations 并触发 attempt 索引。前端收到同 session 的更高 `generation / coveredRevision / newestRevision / newestSeq` 投影时，空 `pendingElicitations` 是权威终态；即使有界 events 页未携带 response event，也不得用旧 pending 覆盖。只有未前进的陈旧 active snapshot 才可保留 live pending。不得为 Elicitation 手动伪造 workflow 续跑；自然续跑仍由 ACP waiter 消费 response、发送 JSON-RPC response 并清理 signal。桌面先处理、IM 后点击时按 AlreadyApplied 收敛但仍 emit 最新投影；并发双入口 first-writer-wins。

权限选项以 ACP `PermissionOption.kind` 为 canonical 语义，提交仍返回原始 `optionId`：`allow_once/allow` 显示“仅允许一次”，`allow_always` 与 `allow_for_session` 显示“本次会话允许”，`reject_once/reject/cancel` 显示“拒绝”，`reject_always` 显示“始终拒绝”。Claude 退出计划模式的 `auto/acceptEdits/bypassPermissions/default/plan` 继续使用 qualifier 映射为“自动模式 / 自动编辑 / 跳过权限 / 手动确认 / 保持计划”。企微标准 Permission vote 按“本次会话允许、仅允许一次、拒绝”展示并默认选中第一项；展示顺序只作用于 presentation，option id 继续携带原 allowed action index。安全拒绝/保持计划仍作为超 20 项时的有界降级候选，不能因展示顺序调整而改变 canonical action 或授权结果。允许类重复且无法区分、未知 kind 或超过控件能力时，完整展示请求但只保留可确认的安全动作或引导桌面端处理。官方 option 无说明字段，不得把说明塞入 `text` 或用展示文案反查语义。

`askUserQuestion`/elicitation 的 message、问题标题、描述、单选/多选类型和 typed 选项只能从当前 `PendingElicitationState.request.requestedSchema` 结构化投影。远程范围仅企业微信：单题 scalar 单选、单题 scalar 多选、2-3 个 scalar 单选字段可发送 IM；自由文本、复合值、混合多选、四题以上、超过控件容量、选项值超界或移除 custom companion 后无法通过原始 schema 校验的请求一律在入队前跳过，不产生 outbox 行、markdown、提示卡、死信或重试，桌面端原生处理不受影响。带 `_askUserQuestionCustomAnswer.isCustomAnswer` 的 companion field 不作为独立问题展示，也不进入 IM 表单；单题单选带 custom answer 时 IM 仍只提交固定 scalar，自定义答案在桌面端填写；2-3 个单选题无论是否存在 custom companion，分类与提交均只基于主问题。IM 卡不提供通用 Decline 按钮，Decline 仍只在桌面端处理。

模板卡片事件只接受官方 `cmd=aibot_event_callback` 且 `body.event.eventtype=template_card_event` 的一对一私聊结构。真实运行时把业务 payload 放在 `body.event.template_card_event` 中，`event_key`、`task_id` 与 vote/multiple 选择必须从该嵌套对象读取；官方 SDK 1.0.7 类型中的平铺字段形状不代表平台实际回调。官方回调中的 `chattype` 可选：显式 `single/private` 为私聊；缺失 `chattype` 且缺失群聊 `chatid` 时按私聊候选处理，以 `from.userid` 作为 conversation/destination identity；显式群聊或存在群聊 `chatid` 时拒绝。`body.msgid` 是平台单次回调幂等 identity；同一 canonical intervention event 后续点击可能携带新的 msgid，Gold Band 必须用入站审计表的 `channel + canonical_event_id` 索引将其收敛为 `ALREADY_APPLIED`。opaque `headers.req_id` 只在内存中随 `ImActionResponseContext` 传递。button 先解析短 `event_key` 取得 delivery identity 与 action index，显式 task 不一致即拒绝，缺失时用该 delivery identity 更新原卡；vote/multiple 先验证 submit key 与必填 task 一致，再解析 question key 与 option id。Permission 的 question key 为 `permission_choice` 且只能返回唯一 option；Elicitation 转换为泛型 Form selection 后由 outbox 的 `RemoteElicitationForm` 还原原始 scalar value。不得用 `req_id` 替代 callback identity，不得生成新 `req_id`，也不得因发送 ACK 没有 `msgid` 而跳过点击响应。解析失败只记录稳定 `parse_reason` 与字段存在性；若私聊回调仍可定位原卡，先尝试把原卡更新为失败态。嵌套 `eventtype=disconnected_event` 进入稳定 `IM_CONNECTION_CONFLICT`，不持续重连争抢。

历史 delivery 中的 destination 只表示发送时接收人，不是后续动作的持续授权事实。每次可操作回调在进入 `InterventionCommandService` 前，必须在 IM settings 写边界内重新读取当前 durable binding，并严格比较 channel、destination、conversation 与 authorized actor；当前 binding 缺失或任一字段变化时返回 `IM_BINDING_REVOKED`，不得 inspect、resume 或修改 Runtime canonical pending state。解绑持久化是授权撤销的线性化点：此前已通过 admission 的动作允许完成，此后进入 admission 的旧卡动作必须拒绝；settings 锁只覆盖读取与比较，不跨 Runtime 执行或网络响应持有。平台已展示的历史内容无法召回，撤销契约只保证旧卡不再执行。

`InterventionCommandService` 的 expected state 校验保持 first-writer-wins，但指纹输入必须与数据所有权一致：pending permission/elicitation 文件中的 `timelineIdentity` 只是展示索引回写，不改变用户待决策的问题、选项、expiry 或 canonical request。指纹序列化显式排除该字段；修改 params/request schema 或允许动作仍必须产生 `INTERVENTION_REVISION_CONFLICT`。

企微 Permission 终态更新帧必须与已验证 POC 保持同形：`main_title.title` 使用“id=xxxx 已处理：<动作>”，`main_title.desc` 为 `Agent 请求执行命令`；同时保持 `vote_interaction`、原 `task_id`、原 `submit_button.key`、`checkbox.disable=true` 和最终选中项 `is_checked=true`。ManualCheck 终态标题同样使用“id=xxxx 已处理：<检查结果>”，并保持禁用 vote、原 task/submit key 和最终选中项，描述使用人工检查摘要。Elicitation 终态必须保持原 vote/multiple 卡：vote `mode=0/1` 均设置 `checkbox.disable=true`，提交项 `is_checked=true`；multiple 每个 `select_list[].disable=true` 并把 `selected_id` 指向最终选择。官方更新协议没有 `submit_button.disable` 字段，Gold Band 不得发送该无效字段；即使客户端仍允许点击提交按钮，重复的新 msgid 也必须在 Gold Band 按 `channel + canonical intervention event` 收敛为 `ALREADY_APPLIED`，不得再次执行 Runtime 或把卡片更新为失败。终态标题把 `display_ref` 放在最前，摘要可截断但编号不可丢失；AlreadyHandled/RevisionConflict 收敛为已处理，业务失败更新失败态但仍保持原结构。若平台拒绝更新 ACK，只记录是否存在 `cmd`、顶层/body 字段名、数字错误码层级和稳定错误码，不记录用户、凭据、文案或完整回调 JSON。

桌面 Runtime 在 blocking 边界必须把 `process_inbound` 返回的 enriched response context 传回对应 connector；业务失败也要保留该 context 并回写 Handled/Expired/Failed 终态。仅当 blocking task 本身 join 失败时，才使用进入任务前克隆的原 channel context 作为 fallback，禁止用无关占位 context 覆盖或在发送更新前丢弃 `req_id/task_id/selected option`。

桌面端完成 Permission、Elicitation 或 ManualCheck 后，也必须从同一 canonical settlement 触发 IM 终态投影。企业微信官方 `updateTemplateCard` 只能使用对应 `template_card_event` 的 callback `req_id`，且更新窗口为收到回调后的 5 秒；桌面操作不存在该上下文，禁止伪造 `req_id` 主动替换原卡。实现从提交前的 pending timeline identity 精确恢复原 intervention event id，使用 `idx_im_outbox_canonical_event` 定位既有 channel/destination/display_ref，并在单个 SQLite 事务中把仍为 pending 的原 delivery 标记 expired、插入 `<source_event_id>:desktop-resolved` 的 typed 非交互终态 delivery。若桌面操作先于异步原请求投影，先按当前 eligible target 写入同一 deterministic 终态 identity；原请求迟到入队时在同一 immediate transaction 内发现该终态并收敛为 Duplicate，不得重新发出可操作卡。原卡已 sending/sent 时不篡改历史，另发 `id=xxxx 已在桌面端处理` / `id=xxxx Handled on desktop` 确认；旧卡后续点击继续由 canonical inbound 幂等收敛。终态确认复用原 destination 和 4 位 `display_ref`；原请求尚未产生编号时使用终态 outbox 自身的稳定 4 位编号。确认不包含 action token、task/submit 控件，也不受已存在原 delivery 后的通知偏好变更影响；重复投影由 deterministic delivery identity 去重。

### 6.2 微信延期门槛

微信不进入首期实现。只有使用真实账号和官方协议完成“客户端未先收到用户消息时仍能稳定主动推送”、休眠恢复、context token 生命周期和长期重连验收后，才可重新评审范围；不得交付需要用户先发送“待处理”的 pull-only 版本。

## 7. 领域与数据所有权

### 7.1 权威事实源

| 数据 | 所属领域 | 权威来源 |
|---|---|---|
| Run/Round/Node/Attempt 生命周期 | Runtime | 现有 canonical 状态文件与 Runtime API |
| 权限请求及其可选项 | ACP/Runtime | 当前 attempt 的权限请求状态 |
| 用户追问及回答状态 | ACP/Runtime | 当前 elicitation 状态 |
| 人工检查等待与结果 | Runtime | 当前 attempt 的 `manual_check_pending` 状态 |
| 平台连接与能力 | IM integration | `ImConnectionManager` 内存状态 |
| 待发送消息、重试和平台消息绑定 | IM transport | 用户级 `core.db` |
| Secret、token、刷新凭据 | Credential store | 操作系统凭据库 |
| 系统原生通知展示与点击去重 | Desktop notification | 现有 `InterventionNotification` / `NotificationDedup` |

IM 数据库不得保存 `approved/failed/completed` 作为工作流事实。它只保存动作请求、提交结果和投递审计；展示最终状态时重新读取领域结果或使用领域提交返回值。

### 7.2 核心模型

```rust
enum ImChannelKind { WeCom }

struct ImChannelCapabilities {
    transport: ImTransportKind,
    proactive_message: bool,
    interactive_card: bool,
    update_card: bool,
    short_text_input: bool,
    single_active_connection: bool,
}

enum ImConnectionState {
    Disabled,
    Connecting,
    Online,
    Backoff { retry_at: DateTime<Utc>, attempt: u32 },
    AuthRequired,
    Error { code: ImErrorCode },
}

struct InterventionLocator {
    project_id: ProjectId,
    task_id: TaskId,
    run_id: RunId,
    round_id: RoundId,
    node_id: NodeId,
    attempt_id: AttemptId,
    outer_node_id: Option<NodeId>,
    outer_attempt_id: Option<AttemptId>,
}

struct InterventionRef {
    intervention_id: InterventionId,
    kind: InterventionKind,
    locator: InterventionLocator,
    request_id: String,
    expected_revision: u64,
    allowed_actions: Vec<InterventionActionKind>,
    expires_at: DateTime<Utc>,
}

struct RemoteInterventionAction {
    action_id: ActionId,
    intervention_id: InterventionId,
    expected_revision: u64,
    actor: ImActor,
    decision: InterventionDecision,
    content: Option<String>,
}

enum ImDeliveryPayload {
    Intervention(InterventionRef, InterventionPresentation),
    Information(InformationalNotification),
}

enum ImNotificationKind {
    Permission,
    Elicitation,
    ManualCheck,
    RunSuccess,
    RunFailure,
    AcpTurnFinished,
}
```

`intervention_id` 从稳定 lifecycle event ID 派生或直接复用；`action_id` 使用平台事件 ID 与 channel ID 形成稳定幂等键。不得用任务名、节点名、IM 文案或短码反查 Runtime 实体。

`Intervention` 是唯一允许创建 action token、卡片按钮和 `InboundAction` 的 payload；`Information` 只携带 canonical event identity、typed notification kind、导航 locator、outcome 与最小摘要，任何回复都不得推进 Runtime。每个符合条件的 `channel + destination` 分别持久化一条 delivery，唯一语义键固定为 `channel + destination + notification_kind + canonical_event_id`。同一 canonical event 重放命中原记录，不同 event ID 即使文案相同也不得聚合。

带 `scheduled_occurrence_id` 的 Run/ACP 信息事件不投递到 IM，scheduled completion、failure、attention 和 missed 只保留既有桌面原生通知。定时任务触发的 Permission、Elicitation 和 ManualCheck 仍按干预事件进入 IM，并继续复用同一 canonical lifecycle、outbox 与入站执行边界。IM 不消费 Windows notification DTO、dismiss 状态或 `NotificationDedup`。

### 7.3 状态转换不变量

1. 每个动作提交前必须按完整 locator 读取当前 Runtime 状态。
2. `expected_revision` 不匹配时拒绝写入，并返回“状态已变化”。
3. 同一 `action_id` 重放返回首次结果，不重复调用领域命令。
4. 同一 intervention 的首个合法终局决策生效，后续竞争动作返回“已处理”。
5. 权限请求只能调用权限响应接口；追问只能调用 elicitation 接口；人工成功/失败只能调用人工检查接口。
6. `manual_check` 只在当前 attempt 为 `manual_check_pending` 且 locator 仍为当前 owner 时接受。
7. 连接 generation 单调递增；旧连接迟到的回调、断线和重连结果不得覆盖新连接状态。
8. connector 内部可发送句柄也由 generation 所有；只有同 generation 可安装或清除 sender，旧会话退出不得清除新会话 sender。
9. `sending` 是有期限的 claim，不是终态；worker settlement 必须携带 claim 时的 `attempt_count` revision，租约恢复后的迟到成功/失败不得覆盖新 claim。
10. 删除配置以 settings 中“不再存在该 channel”为 canonical 事实；跨 settings、投影队列、SQLite 与 keyring 的后续清理由 durable operation journal 推进，失败返回 pending 并由后台重试，不把旧配置复活。

## 8. 设置与连接管理

设置页“通用”页新增“IM 远程干预与通知”区域，按接入生命周期渐进展示企业微信状态。未配置凭据时只提供“扫码接入”；凭据存在后，总开关、状态引导和六项通知始终可见。连接在线但尚无 canonical 私聊 binding 时派生显示“等待绑定”；只有 durable binding 已落盘、delivery target 已重建且当前 generation snapshot 携带同一 binding 时才显示“可用”。该展示模型不落盘。企业微信只提供扫码接入/重新授权，不保留 Bot ID/Secret 手工输入，Secret 不进入前端状态。

Settings schema v12 在严格反序列化前一次性删除 v11 channel notifications 中的 scheduled completion、failure、attention、missed 四个废弃键，并通过现有设置写入边界原子回写。迁移后仍以六字段 `ImNotificationPreferences` 为唯一持久模型；当前版本重新出现废弃键必须报错，禁止恢复兼容读取或旁路消费。

设置区域在正常宽度和窄窗口下保持单列可滚动，平台字段与通知开关允许换行但不得横向溢出；中英文页签使用能在等分 segmented control 中完整呈现的短标签。键盘必须可切换设置页签和所有开关，保存失败通过 `role=alert` 展示 i18n 文案。

### 8.1 设置界面清晰度优化（2026-09-04）

- 根因判断：credential、enabled、connection generation、private binding 与 notifications 的权威状态设计正确；现有界面把不同生命周期阶段平铺成一个表单，属于正确设计下的展示和操作层级不完整，不需要新增 IM 状态机。
- 未接入时只展示“扫码接入”主操作；扫码授权后进入“发送一条私聊”步骤，收到 canonical private binding 后才进入可用态。接入进度完全由既有字段派生，不持久化 UI step。
- 页面顶部只给出一个用户结论：未接入、等待绑定、连接中、正在重连、可用、已暂停、需要重新授权或连接被占用；错误态在原位置给出对应恢复动作，不增加会伪造成功语义的本地测试状态。
- 六类通知仍保持原持久模型，但展示分为“需要你处理”和“执行结果提醒”两组；对客文案不暴露 Run、ACP、安装级目标、协议地址或凭据实现。
- 只有未配置凭据时隐藏通知选项；等待绑定、连接中、暂停、网络错误、鉴权错误和连接冲突时仍允许编辑通知。Bot ID 使用安全断行，不展示敏感凭据。
- 总开关是即时运行命令，目标控件在 100ms 内进入处理中并以后端最新 VM 收敛；通知偏好使用独立本地草稿和窄保存接口，连接事件及启停响应不得清除 dirty 草稿。
- 重新授权、更换接收账号和删除配置进入更多菜单。更换账号与删除分别使用 AlertDialog：前者只清除 binding 和 delivery target，保留凭据、Bot identity、enabled 与通知偏好；后者删除配置、凭据和未发送数据。
后端命令按数据所有权拆分为 `set_im_channel_enabled(kind, enabled)`、`save_im_notification_preferences(kind, notifications)`、`reset_im_channel_binding(kind, expected_generation)`、`reconnect_im_channel(kind, expected_generation)` 与 `delete_im_channel(kind)`。旧通用保存和独立断开入口已经删除，不保留兼容层。snapshot 继续按 generation 单调合并；重连只启动真实新 generation，不提供测试连接。

`delete_im_channel` 先在同一 settings 写临界区创建 cleanup operation、持久化删除配置、移除投影 target，并立即推进 connection/connector generation；之后通过最长 30 秒的 projection barrier 确认旧 target job 已消费，再删除该 channel 的 active outbox 和旧 credential，最后完成 journal。后续清理失败或 barrier 超时时 command 返回已删除 settings、`operationId` 与 `cleanupStatus=pending`，启动阶段和运行期定时恢复按 phase 幂等续做；cleanup operation 存在时拒绝同 channel 重新授权，避免旧清理触碰新配置。

`BindingObserved` 不得直接进入公开 snapshot。connection manager 先校验 generation、私聊属性与既有 actor/conversation；设置写入与命令写入共享最小 IM settings 临界区。binding 可靠落盘并更新 DesktopState 后重建 delivery target，最后才提交同 generation snapshot。存储失败保持 connected-without-binding，并以 `IM_STORAGE_UNAVAILABLE` 启动每个 channel+generation 至多一个、最多三次、1/2/4 秒的可取消重试；generation 前进立即取消，旧结果不得提交。

恢复策略：网络与限流沿用 connector 的有界退避并显示正在重连；鉴权或凭据错误进入重新授权；连接冲突停止自动抢占，用户关闭其他客户端后发起一次真实重连；binding 存储失败保持等待绑定。所有用户文案由前端中英文 i18n 映射稳定错误码。

过度设计复核：不新增 aggregate、持久状态机、全局 store、缓存、轮询、队列或连接测试接口；接入步骤和顶部结论均由现有 canonical 字段派生。性能影响限于单 channel、6 个静态通知项和常数级 DOM 投影；binding retry 有 generation、单任务、次数和延迟上限，不进入高频路径。

企业微信配置包含：

- 是否启用。
- 非敏感标识，如 Bot ID、绑定显示名。
- 投递目标绑定。
- 六个逐类通知开关。默认开启 permission、elicitation、manual check、Run failure；默认关闭 Run success、ACP turn finished。
- 重新授权、更换接收账号、真实重连和删除操作。

UI 不展示协议地址、心跳、轮询、数据库或 token 等实现细节。连接变化只更新设置页的对应平台行，不刷新应用壳、会话历史或工作流详情。

配置保存顺序：先验证非敏感字段，再写系统凭据库的新 reference，再原子写入 `settings.json` 引用，最后触发目标连接 generation 重建。任一步失败都不得留下“设置显示已启用但凭据不存在”的半状态。企业微信 Secret 只存在于扫码轮询的 Rust 响应对象与 keyring 写入调用中，不进入 Tauri response、前端草稿、settings 或 SQLite。

## 9. 安全、隐私与信任边界

- Secret、bot token、access token、refresh credential 和敏感 update token 只进入系统凭据库。
- `settings.json` 仅保存平台类型、公开 ID、credential reference、开关和绑定摘要。
- `core.db` 不保存凭据，不保存完整 IM 对话，只保存投递所需的最小 payload 与审计字段。
- 日志对 credential、context token、用户回答正文和平台原始事件做默认脱敏；排障日志只能记录长度、哈希或结构化 ID。
- 入站动作必须校验 channel、conversation、actor 是否属于已绑定范围。
- 卡片按钮 payload 只携带 `delivery_id:action_index` 短引用，不暴露本地文件路径、完整 locator、action JSON 或凭据。
- action 短引用只允许还原本地 outbox 当前 `allowed_actions` 中的同索引动作，并继续校验 actor、conversation、delivery 生命周期和平台回调幂等。
- 用户回答正文设置明确上限，首期 8 KiB；超限在进入 Runtime 前拒绝。
- 删除平台配置时同步删除 OS 凭据、失效连接 generation，并清理该 channel 的未发送 outbox；审计记录按保留策略清理。

威胁模型不包含“已完全控制本机用户会话的攻击者”。本地数据库仍按 Gold Band 用户目录权限保护，敏感材料额外交给系统凭据库。

## 10. 离线、重试与容量

- 生命周期订阅者只执行 O(1) 转换与有界入队，不执行网络 I/O。
- 桌面启动必须先从现有 settings 同步构建 IM 投递目标快照，再启动后台 bootstrap 并注册 lifecycle subscriber；后台凭据读取、maintenance、连接和后续重配置仍保持异步。初始 settings 读取或目标构建失败时不得注册携带空目标的订阅者，也不得阻断桌面主体启动，错误通过启动日志暴露。
- active outbox 默认上限 1,000 条；达到上限时先小批量清理已过期/终态记录，仍满则拒绝新的远程投递并记录结构化错误与桌面提醒，不得淘汰尚未处理的 active 干预，也不能无界增长。
- 未发送干预在领域请求过期后直接标记 `expired`，不得迟到投递为可操作消息。
- 网络错误使用带 jitter 的指数退避，1 秒起步、60 秒封顶并持续重试，直到连接成功、generation 被取消或出现鉴权失败、连接冲突、配置错误等永久错误；成功建立连接后重置退避。`ReconnectScheduled` 表示重试循环仍存活，`ConnectionFailed` 只表示终态，UI 不得根据错误码猜测二者。
- 平台限流遵守 `Retry-After` 或 SDK 返回的重试时间；单 channel 串行化同一消息更新，其他消息可受控并发。
- `claim_due` 每次只读取最多 32 条候选并在 SQLite 写事务前完成 typed 解析；有效行以 rowid、state、attempt count 做 CAS claim，单条损坏行原子转为 `dead_letter + IM_PAYLOAD_INVALID`，不得把同批健康行改为 sending 后再让整批解析失败。SQLite/锁/事务错误仍是批次级错误。
- 企业微信单 session 最多保留 64 条 pending request。调用端创建的 15 秒总 deadline 随 command 进入 session，由同一 session 使用 `DelayQueue` 在 ACK、发送失败、断连、取消或超时时删除；linked-detail 两帧共享同一 deadline，迟到 ACK 不得重新匹配，达到上限返回 retryable `IM_QUEUE_CAPACITY_EXCEEDED`。
- outbox 已发送/过期记录默认保留 7 天，入站动作幂等记录默认保留 30 天；周期清理采用有上限的小批次。
- 应用启动时先恢复过期发送租约并执行一次 retention；运行期每 30 秒恢复租约、每 6 小时清理 retention。单轮最多 4 批、每批最多 200 条，所有 SQLite 工作在 blocking pool 中执行。
- 应用正常退出时先停止接收新 IM 动作，再取消连接与长轮询，等待有界任务结束；不能因此延迟退出超过设定超时。

客户端退出、休眠或无网络时没有云端在线实体，这是无网关方案的明确产品限制。恢复在线后只补发仍然有效且尚未处理的干预。

## 11. 错误契约

后端只返回结构化错误码和可诊断元数据，不返回对客文案：

```rust
struct ImIntegrationError {
    code: ImErrorCode,
    retryable: bool,
    details: BTreeMap<String, String>,
}

enum ImErrorCode {
    ImConfigInvalid,
    ImCredentialUnavailable,
    ImAuthenticationRequired,
    ImConnectionUnavailable,
    ImPlatformRateLimited,
    ImBindingRequired,
    ImBindingRevoked,
    ImActorForbidden,
    ImActionInvalid,
    ImActionExpired,
    ImActionAlreadyHandled,
    ImRevisionConflict,
    ImRuntimeStateMismatch,
    ImQueueCapacityExceeded,
    ImPlatformProtocolError,
}
```

前端根据稳定错误码、企业微信出站 renderer 根据稳定 presentation key 使用 i18n 资源映射中文与英文文案。平台原始错误只能作为脱敏诊断字段，不能穿透到用户界面。

## 12. 验收标准

### 12.1 领域正确性

- 桌面端与 IM 对同一请求并发操作时只有首个合法动作生效。
- 过期、旧 revision、错误 actor、错误 locator 和重复平台事件均不能改变 Runtime。
- 人工成功/失败不能用于非 `manual_check_pending` attempt。
- IM 连接、数据库或平台故障不影响 Runtime 正常执行和桌面端本地干预。

### 12.2 平台行为

- 企业微信可连接、绑定全局一对一目标、接收三类干预与已启用信息通知、操作卡片并更新终态。
- 微信 connector 和设置入口保持缺席，直到稳定主动推送真实 PoC 通过。
- 客户端重启后恢复启用配置和未过期 outbox，但不复活旧连接状态或已处理动作。

### 12.3 安全与隐私

- 仓库、workspace、`settings.json`、`core.db`、日志和前端 state 中均无明文 Secret/token。
- 未绑定会话、错误 actor 以及不再匹配当前 durable binding 的历史卡动作被拒绝且不改变 Runtime。
- 解绑、更换接收账号或删除配置后，旧卡片不可继续操作；删除后凭据不可再读取。

### 12.4 性能

- lifecycle publish 路径不等待网络，入队为 O(1) 且有界。
- lifecycle projection 的 blocking task、repository/validation 结果必须形成可观测 completion；只有持久化完成后才可唤醒 worker，单条失败不得终止后续投影。
- 正常运行最多维护一个企业微信 WebSocket。
- 不扫描完整 timeline、全部会话或历史 run 来构造通知。
- 设置状态更新仅重渲染对应集成区域。

## 13. 方案审视

### 13.1 根因与完整性

现有 Runtime 的 canonical identity、revision 和生命周期足以表达审批与人工判定。方案修复跨通道应用服务缺失的问题，并把 Tauri 与 IM 的写入口收敛到同一接口；不复制 Runtime 数据模型，也不通过名称或消息内容旁路定位。

2026-08-30 企微三类通知统一死信的根因是连接器偏离官方主动推送契约：出站 body 多发 `chat_type`，且 ACK 解析把未承诺的 `body.msgid` 误设为成功前置条件。事件发布、逐类策略、私聊 binding、outbox 入队和 worker 唤醒均已有数据库证据证明正常，因此修复协议 adapter，而不增加 lifecycle 旁路或第二套通知状态。

2026-08-31 重启后新 Run success 与 elicitation 均未产生 outbox 行。运行日志证明 canonical lifecycle event 已被同总线的 scheduled subscriber 接收，配置快照证明企微、凭据、私聊 binding 与逐类开关有效；旧 outbox 最晚生成于 22:42，而故障进程于 23:52 启动，因此旧进程的入队证据不能外推到新事件。Run success 的直接根因是 canonical event ID 缺少 `task_id`：不同 task 都从 `run-001/round-001/attempt-001` 开始，新事件与旧 task 的 event ID 完全相同，被正确的语义唯一约束判为重放。修复 canonical identity 生成器为 `project + task + run + round + node + attempt + event-specific identity`，不削弱 dedupe。elicitation 的直接根因是领域 owner 校验错误地要求 Run 未 completed，忽略 Direct 首轮完成后同一 ACP attempt 仍可产生 pending permission/elicitation；修复后 ACP pending request 由当前 locator + canonical pending request 判定，manual check 仍严格拒绝 completed Run。

IM runtime 的 tracing 必须先于连接、配置加载和 lifecycle subscriber 启动，以便稳定区分“没有投影”“投影失败”“outbox 发送失败”和“平台拒绝”。该顺序只补齐可观测性，不新增业务状态或旁路发送；真实平台验收必须以新 canonical event 的 projection completion、outbox 终态和用户实际接收三项证据共同确认。

Connector task 结束时必须先排空其有界事件队列，再把返回错误投影为同 generation 的 terminal `ConnectionFailed`；retryable 网络错误必须在 connector 内发布 `ReconnectScheduled` 并继续有界退避，不能返回伪终态。不得 abort 转发器或忽略 `connect()` 返回值。连接日志只记录 channel、generation、稳定错误码、retryable 和平台数字码，不记录 Bot ID、Secret、目标、payload 或平台错误文案。

2026-08-31 IM 卡片展示与点击问题属于正确领域设计下的 connector/projection 实现不完整，而非 Runtime 状态模型缺陷：elicitation parser 只识别“恰好一个 scalar oneOf”，权限 label 又未覆盖实际 `allow_once/allow_for_session/cancel`；企微 callback 错把不存在的 `body.event_id`/`req_id` 当幂等 identity，并用新 `req_id + message binding` 更新卡片。修复保留 Runtime canonical state 与 `InterventionCommandService`，补齐 typed question projection、稳定语义文案和 transient callback response context，不新增 SQLite 业务状态或兼容入口。

2026-08-31 真实权限卡信息不全与按钮无反馈的后续复核确认两个实现缺口：`inspect_permission()` 未把 pending params 投影为 prompt，导致卡片只有任务/节点；官方 SDK 类型定义中 `chattype` 可选，但 connector 把缺失该字段的真实单聊回调判为协议无效。修复仍在既有 pending state、projection 和 inbound action 边界内完成：权限生成结构化摘要并按平台容量安全降级，企微按“缺失 chattype 且无 chatid”识别私聊，并为可定位原卡的解析失败排队失败态更新；不新增第二套展示状态、回调状态机或平台消息缓存。

2026-09-01 真实日志确认企微点击事件已经到达客户端，但被 `EVENT_KEY_MISSING` 拒绝。直接把完整 action JSON 与 HMAC token 塞入 key 属于连接器边界设计缺陷，改为短引用并由本地权威 outbox 还原仍是正确修复；但后续复核官方 SDK issue #22 的真实回调样本确认，平台并未丢弃 `event_key`，而是把它嵌套在 `event.template_card_event` 中，SDK 1.0.7 类型定义与运行时形状不一致。最终根因是协议 adapter 依据错误类型形状实现解析，不新增双路径兼容，直接按真实嵌套结构读取 `event_key/task_id`。同日复核确认按钮截断不是 2 字符协议上限，而是官方没有排列控制时 3 按钮横排的展示约束；修复为最多 2 个 typed 优先动作。

同次回溯还发现异步桥接实现不完整：projection consumer 同时吞掉 blocking JoinError、repository/validation error 和 intervention inspect error，并在持久化前提前唤醒 worker。修复以每 job 的结构化 completion 收敛错误和时序，不增加历史扫描、旁路发送或第二套业务状态。

2026-09-01 嵌套回调修复后的真实复测证明点击已进入 Runtime，但统一被 `INTERVENTION_REVISION_CONFLICT` 拒绝。pending permission 先被 IM projection 读取，随后 ACP client 回写 `timelineIdentity`；旧实现把完整 pending 文件纳入 expected state，误把展示投影 revision 当成决策状态变化。根因属于正确 CAS 设计下的指纹契约实现错误，修复为显式序列化决策语义并同步覆盖 elicitation，不放宽冲突校验或回改 outbox。企微随后返回 42045 的独立根因是更新帧把 `button_interaction` 改成缺少 `button_list` 的 `text_notice`；官方更新示例要求保持 `button_interaction`、`button_list` 与原 `task_id`，因此修正协议帧而不是增加回调配置状态。

2026-09-01 Permission 按钮截断的最终修复属于正确领域设计下的企微 presentation adapter 实现不完整：canonical pending permission、expected state、outbox allowed actions、原始 optionId 提交与 `InterventionCommandService` 均保持不变，仅把企微 Permission 投影为官方 `vote_interaction` 并按真实回调契约还原唯一选中项。真实 PoC 证明手机端与 PC 端回调均返回 `selected_items`，且不同选择对应不同 `option.id`；平台更新约束为必须保留 `submit_button`，否则返回 42049。未知 kind、允许类语义重复、非法 index、task 不一致、未选择或多选均走稳定失败/桌面处理路径；重复 msgid 仍复用入站幂等账本。

2026-09-01 真实复测反馈的“详情不可见”和“终态未置灰”仍属正确领域设计下的 presentation/协议投影缺口：官方横向字段 26 字建议无法承载完整命令与说明，生产终态又缺少与已验证 POC 相同的 `main_title.desc`。修复把企微 Permission 拆为同一 delivery 的详情 markdown 与 vote 卡两帧，并在详情 ACK 后才发送卡片；`cwd` 进入结构化路径。终态先与 POC 同形，并在 ACK 拒绝时输出脱敏字段形状诊断。该路径不改变 Runtime 决策、outbox 权威动作或入站幂等语义。

2026-09-02 权限通知“偶发不达”的根因是 canonical identity 实现把 transport-scoped JSON-RPC request id 误用作 permission occurrence identity：真实 raw 帧证明 `session/resume` 后 `requestId=0` 重置，同一 attempt 中 `Write kelvinzhou.txt` 与 `Write weiqi.txt` 的 toolCallId 不同却生成相同 lifecycle event ID，outbox 按语义唯一约束正确判定 replay 并跳过。修复为分离 pending/response 查找用的原始 requestId 与 timeline/lifecycle 用的 Gold Band permission occurrence identity；不新增重试、时间窗、第二套 outbox 或企微旁路发送。缺 toolCallId 时使用已持久化 sequence 保持有界身份，不使用展示文案或时间戳猜测授权语义。

2026-09-03 Elicitation 远程表单缺口属于正确的 canonical pending/requestedSchema 设计下，远程投影与应用收尾实现不完整：既有架构已提供权威请求、outbox、delivery identity、msgid 幂等、expected state 与共享命令服务，但远程动作仍被投影成单题单选按钮，企微缺少 multi-select/multiple 控件适配，IM 入口也没有复用桌面的 scheduled reclaim、metrics cause、session 投影和 attempt 索引收尾。修复方向是补齐 channel-agnostic fixed form 契约与企微表单 adapter，并统一 Elicitation 应用执行边界；不新增第二套审批状态、第二 outbox、草稿状态机或多卡片向导。

2026-09-03 真实复测发现“终态后仍可重复点击”不是首次执行失败：outbox 与 inbound 成功记录证明 Runtime 首写正确，重复点击携带新的 msgid，且 ACP waiter 已按设计清理 pending/response signal，旧实现随后把迟到点击误判为 RequestNotFound/RuntimeStateMismatch。另一复核确认企微更新协议支持 checkbox/select disable，不支持 submit_button.disable。修复保留原控件与 task/submit key，删除无效 submit disable 字段，并为入站审计增加 canonical intervention event 索引；Permission/Elicitation 的桌面先处理场景另从 durable timeline response/request 恢复 AlreadyApplied。

2026-09-04 可靠性复核确认七处问题均属于已有设计正确但生产编排或消费契约未闭环：repository 已有租约恢复和 retention API 却没有生产调度；manager 已按 generation 拒绝旧事件但 connector sender 没有相同 ownership；删除跨三个存储却仍按一次性顺序执行；重配置循环把单 channel 故障提升为全局失败；前端仍用 transport requestId 覆盖 occurrence identity；有界投影队列满载时回退到 publisher 同步磁盘 I/O。修复分别补齐 claim revision fencing、maintenance worker、generation-owned sender、durable cleanup journal、逐 channel 错误收敛、canonical event key 和 O(1) 满载诊断，不修改 Runtime 审批模型或增加旁路发送。

2026-09-07 四项 IM 修复分别补齐授权 admission、坏行隔离、请求 deadline 所有权和连接生命周期契约。历史 delivery 的 actor 只是发送快照，当前 durable binding 才是执行授权；outbox 的问题来自 claim 提交早于 typed decode；pending 泄漏来自外层 timeout 与 session map 所有权分离；错误 UI 来自 retry progress 与 terminal failure 共用 `Disconnected`。修复复用 settings、dead letter、Tokio deadline、generation 和 connection manager，不修改 Runtime 审批模型。

2026-09-07 启动期通知丢失属于正确异步 bootstrap 设计下的 readiness 边界缺失：runtime 初始目标为空，lifecycle subscriber 却在异步 `reconfigure` 完成前开始接收事件，导致事件把空目标固化进投影 job。修复以现有 settings 为权威源，在订阅前同步建立目标快照；异步 bootstrap 继续负责连接、维护和再次读取最新配置，不新增 readiness 状态机、事件回放或第二套目标事实源。

### 13.2 过度设计评审

首期不引入云端网关、Kafka、通用事件平台、独立 helper 进程、新审批 aggregate 或自然语言 Agent。新增的连接管理器、outbox 和幂等表分别对应长连接生命周期、进程离线重试和外部事件至少一次投递三个真实不变量，复杂度与风险相匹配。

2026-09-04 可靠性闭环不新增通用 scheduler、第二 outbox、配置 revision、租约表或前端 canonical 副本。现有 `state + next_attempt_at_ms + attempt_count` 已能表达 claim lease，现有 connection generation 已能表达 sender ownership，现有 `AcpUiEvent.id` 已是 occurrence identity；只新增 cleanup journal，因为 settings、SQLite 与系统凭据库无法共享事务，而删除必须具备可恢复进度。

2026-09-07 修复不新增 binding epoch、撤销表、outbox 状态、数据库迁移、重连 supervisor 或外部依赖。现有 durable binding 足以表达当前授权，现有 `dead_letter + last_error_code` 足以隔离坏行，已有 `tokio-util::DelayQueue` 足以统一 pending deadline；`Reconnecting` 只是连接管理器的 transient lifecycle 状态，不形成第二份持久事实。

2026-08-31 权限/企微回调补齐不新增展示事实源：`InterventionPrompt.fields` 是 pending params 的一次性 transport projection，按钮容量降级发生在 IM projection/connector 边界，callback 失败态仍复用 outbox 与原 delivery identity。现有 canonical pending state、delivery ID、msgid 幂等和 `InterventionCommandService` 已能表达不变量，因此不为平台容量或解析失败增加第二套状态机。2026-09-01 的短 action 引用与 vote option id 同样只复用 delivery ID 与 outbox action index，不新增映射表、缓存或第二套 token；`ImWeComVoteSelection` 是回调到终态更新之间的一次性 transport context，成功更新后即释放。Permission 双发只新增 connector 内存中的“详情 ACK 后发送卡片”pending 分支，不新增第二条 outbox、平台消息 identity 或持久状态。

2026-09-02 permission occurrence identity 只是把原本隐含在 timeline item ID 中的身份规则显式化：provider 原始 requestId 仍是唯一 Runtime request identity，新增 hash 只服务于 timeline upsert 与 lifecycle/outbox 幂等，不改变审批 aggregate、expected state 或动作提交模型。

2026-09-02 ManualCheck 远程卡片缺口属于正确 Runtime 设计下的 presentation 上下文不完整：`manual_check_pending`、`submit_manual_check` 与 allowed actions 已能表达唯一人工判定，但 snapshot 未携带用户决策所需的最新模型输出，企微也仍使用两个按钮。修复只在 ManualCheck 投影中加入有界模型输出快照，并复用 Permission 已验证的“markdown 详情 -> ACK -> vote 卡 -> ACK”与 4 位 `display_ref` 配对契约；不新增审批状态、第二条 outbox 或展示文案反查。

2026-09-02 ManualCheck“卡片可提交但 Run 不续跑”的根因是共享 `InterventionCommandService::execute` 对 ManualCheck 返回状态错误，而桌面 command 另行走 prepare/resume/commit 两阶段路径，形成入口漂移。修复把桌面与 IM 都收敛到同一个 ManualCheck 应用执行边界，并保留 scheduled occurrence、conversation callbacks、后台 launch ack 与 first-writer lease；这是正确设计下的共享入口实现补齐，不是新增第二种审批流程。

2026-09-02 Permission“IM 已处理但桌面权限卡残留”的根因不是权限响应写入不同：两条入口都使用共享 `execute` 写 response。差异在桌面 command 在 execute 后重建并 emit ACP session 投影，IM 只写 response 未通知前端刷新 pending permission。同轮终态卡片重复可点击与标题丢失 id，分别来自企微协议不支持禁用 `submit_button`、回调上下文未携带 durable `display_ref`。修复统一 Permission 应用执行/收尾边界，终态只禁用协议支持的 checkbox/select，并在标题前置保留 4 位配对编号；这些均为正确领域设计下的 presentation/application 边界实现补齐。

2026-09-03 Elicitation 表单契约只把既有 pending schema 中可安全远程表达的 typed 事实固化为 `RemoteElicitationForm`，并复用当前 delivery 的 allowed action、expected state、outbox 原始值和原始 schema 校验。selector key 与 option id 是平台回调短引用，不新增映射表、缓存、token 或第二身份；unsupported 场景在入队前跳过，不产生平台消息或死信。

2026-09-03 canonical 入站幂等只复用已有 `im_inbound_actions` 审计账本和 canonical event identity，不复制 Runtime 决策、不修改 outbox 状态，也不为每张卡片新增第二张动作表。前序输出是 pending request 投影时的有界 presentation snapshot，不参与 expected state 或 CAS 指纹。

2026-09-03 桌面终态真实复测失败属于企微 connector 的发送分流与局部错误隔离实现不完整：`notification_kind` 只能描述业务类别，不能单独决定 transport 形状；同一个 Elicitation kind 下，待处理 `Intervention` 需要“详情 + 交互卡”，已处理 `InterventionResolution` 只允许非交互 markdown。旧实现仅按 kind 选择 linked-detail，终态 payload 因形状不匹配在发送前返回 `IM_PROTOCOL_INVALID`，该单 delivery 构建错误又退出整个 WebSocket session，导致 terminal outbox 持续得到 `IM_NETWORK_UNAVAILABLE`。发送策略现同时检查 payload 生命周期与 kind；单 delivery 的构建/校验错误只完成该请求的失败结果并继续 session，只有 socket、订阅或平台连接冲突可以改变 channel 连接状态。canonical 审批、terminal outbox 与桌面来源无法伪造 callback 更新原卡的协议结论保持不变。

### 13.3 性能影响评审

最多一个企业微信 WebSocket；队列、重试、并发、记录保留均有界。单次 lifecycle 投影为 `O(C)` 且 `C <= 1`；permission 摘要只读取当前 pending request 一次，复杂度为 `O(P + F + A)`（params 条目、投影字段与动作数），路径最多展开 3 条、标题/正文/字段各 256 字符；企微 Permission 双发是当前 delivery 的两个出站帧，详情渲染 `O(F)`、vote 排序仍是 `A <= 20`；elicitation schema 投影为 `O(Q + O)`（问题数加选项数），二者均不扫描 timeline 或历史 Run。回调只解析短 option id/action index，按 delivery ID 做一次 indexed 读取，并从当前 outbox 权限动作列表还原原始 optionId；终态更新前的 transient 动作克隆不超过 20 项。Runtime/SQLite 工作在 blocking pool 内完成后才发网络响应，网络不位于 SQLite transaction 或领域锁内；热路径不增加全量加载、N+1、无界缓存、长锁或事件线程阻塞。completion 仅携带 canonical event ID、稳定错误码与三个计数，worker 唤醒由落库前移到落库后并减少空 claim。实施阶段需通过接口测试固定队列容量、批量清理和连接 generation，不要求在未出现吞吐瓶颈前引入 benchmark 或并行发送优化。

Permission occurrence identity 的生成只对当前事件的两个短字符串做一次长度前缀 BLAKE3 哈希，输出固定 64 hex，避免 provider ID 长度进入 lifecycle key；pending 查找仍是按原始 requestId 的一次文件读取，未增加扫描、缓存或额外 I/O。

2026-09-02 ManualCheck 补充的模型输出读取使用 timeline index：索引扫描只过滤当前 root、可见、非 Agent launch 的 `textDelta` locator，并按最新顺序读取事件到第一个非空输出；输出快照 4096 字符有界，不加载完整 timeline，也不引入缓存。企微双发仍是当前 delivery 的两个出站帧，ManualCheck vote 排序固定为 2 项。

2026-09-03 Elicitation 分类与渲染复杂度为 `O(Q + O)`，Q 不超过 3，vote 选项不超过 20，multiple selector 不超过 3 且每题不超过 10；不枚举多选组合，不扫描 timeline 或历史 Run。回调只解析短 selector/option id，做一次 indexed delivery 读取和一次 schema 校验；每个支持 delivery 仍是两个出站帧，unsupported 场景入队前跳过且不产生重试、死信或平台消息。

2026-09-03 重复新 msgid 点击增加一次 `channel + canonical_event_id + completed_at` 索引查询；命中后只写一条 AlreadyApplied 审计结果，不进入 Runtime、scheduled resume 或 session 重建。Elicitation 前序输出读取复用 timeline index 的最新 root 文本定位并最多读取一个事件、快照 4096 字符；详情去重只比较当前请求内的 message/description/context。

2026-09-03 桌面终态发送分流只增加一次 enum variant 判断，为 O(1) 且不增加网络帧、SQLite 查询、锁、队列或缓存。局部构建失败在写 socket 前完成对应 oneshot，不占用 pending map，也不触发重连或后续 outbox 的网络失败风暴；正常终态仍为单条 markdown。

2026-09-04 maintenance 的租约扫描和 retention 删除均命中既有索引，单批 200、单轮 4 批；周期分别为 30 秒与 6 小时，不执行 VACUUM 或全量 payload 加载。投影队列 Full 分支只做常数级状态判断、结构化日志和 30 秒冷却的诊断事件，不再执行 Runtime 查询、文件读取或 SQLite 写入。projection barrier 只用于低频删除清理，且不持有 settings/SQLite 锁；重配置按 channel 顺序执行但单个失败不阻断后续 channel，当前仅一个 channel，不引入无收益并行化。

2026-09-07 `claim_due` 的 typed decode 仍受 32 条批次和 32 KiB payload 上限约束，并移到 SQLite immediate transaction 之前；事务只做有索引候选的 rowid/CAS 更新。每次入站动作增加一次小型 settings 读取，settings 锁在 Runtime 执行前释放。pending map 与 `DelayQueue` 同为最多 64 条，超时删除为 `O(log 64)`；空 queue 分支不参与 `select`，不会空转。持续重连最多一个 WebSocket，间隔 60 秒封顶且 generation cancellation 可立即停止，不引入全量扫描、无界内存、长锁或紧密重试。

## 14. 外部协议依据

- 企业微信智能机器人 WebSocket：[官方文档](https://developer.work.weixin.qq.com/document/path/101463)
- 企业微信智能机器人官方 Node SDK：[`WecomTeam/aibot-node-sdk`](https://github.com/WecomTeam/aibot-node-sdk)
- 企业微信官方 CLI 扫码授权实现：[`WecomTeam/wecom-cli` `auth/qrcode.rs`](https://github.com/WecomTeam/wecom-cli/blob/main/crates/wecom-cli/src/auth/qrcode.rs)
- 微信官方接入包：[npm `@tencent-weixin/openclaw-weixin`](https://www.npmjs.com/package/@tencent-weixin/openclaw-weixin)

实施时必须再次核对官方权限、限流、回调时限和版本变更，并把依赖精确锁定到审计通过的版本。
