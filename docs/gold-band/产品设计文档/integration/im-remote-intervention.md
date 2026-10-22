# 客户端直连 IM 的远程干预设计

## 1. 文档状态

- 状态：已实施（功能开发完成；真实平台剩余验证项独立跟踪，不等同于发布验收全部关闭）
- 当前正式接入企业微信与飞书；2026-10-15 用户确认 P2 验收通过，P3 三端终态互通与 P4 设置页/七语言已实现。微信延期，等待稳定主动推送真实 PoC。历史验证证据见[飞书技术方案 §15](../../开发计划/IM接入/飞书远程干预技术方案.md#15-测试矩阵)，当前验收状态统一见[§17.1](../../开发计划/IM接入/飞书远程干预技术方案.md#171-当前验收状态)。
- 2026-10-15 验收修正（用户确认）：企业微信多题单选最多 3 题，四题及以上在投影前整体跳过，不能只发详情文本；飞书保留原卡片组件、已选答案和输入内容，并禁用控件。其后用户已确认 P2 验收；技术方案继续记录 P1 未测项与平台边界，不据自动化测试宣称整体验收完成。
- 历史（已由当前双渠道范围替代）：2026-09-03 范围收缩：删除第二平台 connector、SDK、通道枚举、设置接口和 UI，不保留隐藏入口或兼容消费路径
- 2026-09-04 可靠性闭环：补齐发送租约恢复、保留清理调度、generation-owned connector sender、删除清理 journal、局部重配置失败隔离、前端 occurrence identity 与投影队列背压契约
- 运行形态：Gold Band 桌面客户端进程内直连，不部署 Gold Band 云端 IM 网关
- 关联开发计划：[客户端直连 IM 远程干预实施计划](../../开发计划/IM接入/客户端直连IM远程干预实施计划.md)
- 文档职责：本文维护当前产品行为、能力边界与业务不变量；实施计划维护模块、接口与迁移；飞书技术方案保留选型、PoC、实施历史与验收证据。各文档标明日期的历史记录不作为当前契约。
- 2026-10-16 首次文档核对修正旧表单类型、selector 与空多选语义；随后用户确认落实协议封装与卡片类型化，两项已按原设计目的修正代码，当前契约见 §16、验收见[飞书技术方案 §17.3](../../开发计划/IM接入/飞书远程干预技术方案.md#173-设计与实现偏差收口)。真实平台待验收项仍独立跟踪。
- 2026-10-16 17:20 自动化修复版本：包含后续 IM 修复的当时主工作区完成验证，Rust 46 个目标、2871 通过、0 失败、18 ignored；前端 376 个文件、2861 通过，Node 脚本 157 通过，生产构建、现有 CI Clippy 门禁与格式检查通过。见[自动化回归修复记录](../../开发计划/IM接入/自动化回归修复记录-2026-10-16.md)；[首次快照失败记录](../../开发计划/IM接入/自动化回归记录-2026-10-16.md)保留为历史。后续重构的验证见[封装与类型化实施记录](../../开发计划/IM接入/飞书封装与类型化实施记录-2026-10-16.md)，此前全仓结果不覆盖新重构。用户确认的真实场景与安装包、平台边界分别按飞书技术方案 §17.1 跟踪，自动化通过不关闭发布验收。

## 2. 一句话定义

Gold Band 在桌面客户端内连接用户自己的企业微信或飞书机器人，把已有工作流干预和用户启用的 Run、ACP turn 信息通知投递到安装级全局一对一私聊；可操作请求经身份、状态与幂等校验后提交回现有 Runtime，信息通知始终只读。

## 3. 问题与设计结论

当前 Runtime 已经拥有权限请求、用户追问和人工结果判定的权威状态与提交接口，缺口是桌面端之外没有可达的通知和操作通道。因此问题不来自 Runtime 的根本设计缺陷，不需要重做工作流状态机；需要补齐的是统一的远程干预应用服务、平台连接器、可靠投递和安全凭据边界。

设计结论：

1. `RuntimeLifecycleBus` 是待干预事件的来源，Runtime 持久状态始终是唯一事实源。
2. IM 只是输入输出通道，不保存或推导第二套审批状态。
3. Tauri UI 与 IM 共用同一个 `InterventionCommandService`，不得互相调用外层 command。
4. 连接器只处理平台协议，平台 SDK DTO 不进入领域层。
5. 首期只使用平台交互卡片的明确控件提交动作（按钮、选择器或表单），不做自然语言 ChatOps。

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

- 在设置页配置、授权、启停和查看企业微信、飞书连接状态。
- 工作流进入权限审批、用户追问、人工检查等待态时发送通知。
- 在 IM 中同意或拒绝权限请求。
- 在 IM 中回答用户追问。
- 仅当当前 attempt 处于 `manual_check_pending` 时判定成功或失败。
- 展示客户端在线、授权待处理、退避重连、断开等连接状态。
- 支持离线期间的有界可靠投递、去重、过期与审计。
- 按逐类开关投递 Run 和 ACP turn 信息通知。
- 企业微信与飞书均使用交互卡片；每个渠道只绑定一个机器人、一个授权用户和一个一对一私聊 destination。一个机器人只连接一台设备。
- 任一端处理成功后，另外两端按 §6.4 收敛终态；只有 Accepted 的赢家写处理来源与结果。

### 4.2 非目标

- 不提供 Gold Band 托管网关、跨设备消息中继或云端凭据托管。
- 不把 IM 会话当作完整的工作流控制台或聊天历史镜像。
- 不支持任意自然语言解析、模型参与的指令判断或模糊匹配。
- 不支持通过 IM 启动、停止、重跑、编辑工作流或执行任意命令。
- 不支持群聊通知、群聊按钮或群聊文本指令。
- 微信在真实 PoC 证明稳定主动推送前不提供 connector、设置入口或 pull-only 降级。
- 不保证客户端退出、电脑休眠或断网期间仍能实时收发消息。

## 5. 平台能力与产品降级

| 能力 | 企业微信 | 飞书 |
|---|---|---|
| 官方直连 | 智能机器人 WebSocket | pbbp2 WebSocket 长连接 |
| 接入 | 扫码取得机器人凭据 | 扫码创建应用或重新授权已有应用 |
| 绑定 | 首次授权私聊交互 | 仅扫码人发来的首次私聊消息 |
| Permission | 最多 20 项 | 最多 20 项，按钮或单选下拉 |
| ManualCheck | 成功/失败投票 | 成功/失败按钮 |
| 单选 | 单题 20 项；2–3 题每题 10 项 | 最多 5 题，每题 20 项 |
| 多选 | 单题，最多 20 项，不支持带“其他” | 最多 4 项，可与其他题型混排 |
| 文本 / “其他” | 不支持文本；单选只发固定选项 | 单行/多行 500 字符；“其他”100 字符 |
| 拒绝回答 | 不发布该动作 | 支持 |
| 终态更新能力 | CallbackOnly：仅点击后 5 秒回调内更新；跨端另发文本 | Anytime：2.5 秒内回调，另由 outbox PATCH 原卡 |
| 群聊 | 不支持 | 不支持 |

飞书数值是当前已验证的保守能力，不代表平台理论极限。超出能力的表单整单不投递到该渠道，其他渠道和桌面端照常处理，不拆分请求或静默丢题。微信不提供 connector 或设置入口。

上层只按 `ImChannelCapabilities.interaction` 和 `message_update: None | CallbackOnly | Anytime` 投影，不按平台名分支。领域 `RemoteElicitationForm` 复用 `ElicitationQuestion`，只限制渠道无关的 schema 安全约束；`fit_form` 决定每个目标实际发布的题目与动作。入站仅接受该 delivery 发布的动作。无歧义 Permission 超出上限时只保留安全拒绝类动作；无法安全处理的请求交回桌面端。

## 6. 用户流程

### 6.1 企业微信

1. 用户在设置页点击“扫码接入”，客户端从企业微信官方扫码授权端点取得短期二维码；用户使用手机企业微信扫码并确认授权。
2. Rust 后端轮询授权结果，取得 Bot ID 和 Secret 后直接写入操作系统凭据库；前端只接收二维码 URL、到期时间和脱敏状态，不接收 Secret。
3. 客户端连接 `wss://openws.work.weixin.qq.com`，连接成功后展示“在线”。
4. 用户先在目标会话中与机器人交互，Gold Band 记录允许主动投递的会话绑定；完成前设置页持续显示“等待绑定”并提示用户发送一条私聊消息，不把 WebSocket 在线误报为可投递。企业微信单聊事件没有 `chatid` 时，以 `from.userid` 作为 conversation/destination identity；不得因此退回群聊绑定。
5. Runtime 发布待干预事件，企微 Permission、ManualCheck 与支持的 Elicitation 先发送一条 markdown 详情消息，再发送交互卡。详情固定展示工作空间、任务、节点和请求内容；卡片只承载请求摘要、类型专属字段、到期时间和允许动作，不重复展示这三个上下文字段。两条标题携带同一 4 位 `display_ref`，详情 ACK 前不得发送卡片，卡片 subtitle 明确指向上一条详情。
6. 用户提交卡片动作后，连接器从官方回调结构中取得 `body.msgid`、`headers.req_id`、typed `event.template_card_event.event_key/task_id`；Permission vote 卡还必须取得唯一 `selected_items.selected_item[].option_ids.option_id[]`。Runtime 提交在 blocking pool 内完成，并须在平台 5 秒窗口内返回卡片更新。
7. 领域提交成功后使用同一回调的 `req_id` 与原卡 `task_id` 更新卡片为最终结果；显式返回的 `event.template_card_event.task_id` 必须等于 `event_key` 携带的完整卡片标识。button 回调省略可选 task 时可从 `event_key` 恢复原卡标识；vote 回调的 task 为必填契约。若请求已被桌面端或另一消息处理，则显示“已处理”。发送 ACK 是否返回平台消息 ID 不影响这条回调更新路径。
8. IM outbox 固化的 `expected_state` 只包含决策语义：完整 locator、canonical request、原始 params/request schema、创建时间与完整允许动作。`timelineIdentity` 属于 Runtime 展示投影元数据，可在 outbox snapshot 之后绑定，不参与 CAS 指纹；ManualCheck 详情中的模型输出同样是投影快照，不参与 CAS 指纹。企微三类干预终态都必须保持原 `card_type`、原 `task_id`、原 `submit_button.key`，显式禁用协议支持禁用的 `checkbox` / `select_list` 并标记最终选择；官方 `submit_button` 类型没有 `disable` 字段，不得发送无效字段，也不得把可交互卡降级为只有标题的 `text_notice`。平台仍允许再次提交时，由入站 canonical event 幂等保证不重复执行 Runtime。

企业微信扫码协议复用官方 CLI 的 `/ai/qc/generate` 与 `/ai/qc/query_result`，3 秒轮询、5 分钟到期；安装级 `source` 位于 `configs/app-config.toml`，当前明确为 `maling`。跨企业授权能力仍须使用真实企业账号完成验收，不得仅根据可生成二维码宣称验收通过。企业微信心跳默认 30 秒；卡片回调的协议响应目标小于 5 秒，领域处理不得阻塞回调确认。同一机器人被其他客户端的新长连接替换时进入稳定连接冲突错误，不持续抢占重连。

主动推送严格对齐官方 `@wecom/aibot-node-sdk` 的 `aibot_send_msg` 帧：body 只包含 `chatid + typed message payload`，不得附加未定义的 `chat_type`。WebSocket 帧分发以是否携带 `cmd` 为第一契约：带 `cmd` 的帧是事件或业务请求，即使 `headers.req_id` 与 pending request 相同也不得消费回执；只有无 `cmd` 的帧才允许按 `req_id` 匹配 ACK。投递成功以存在且为数字的回执顶层 `errcode == 0` 为准，缺失或错误层级不得默认成功；官方回执未承诺 `msgid`，因此 ACK 可将 delivery 标记为 sent，但只有平台真实返回消息 ID 时才写 `im_delivery_bindings`，禁止构造伪平台消息 ID。未知平台错误只在脱敏诊断中保留数字码，对客和 SQLite outbox 仍只保存稳定错误码。

企业微信模板卡片不得把 Markdown 文档塞入 `main_title.title`。标题、描述、横向字段和控件文案分别按官方 26/30、5/26、10/11 字建议上限有界渲染；官方协议最多支持 6 个横向字段。Permission 与 ManualCheck 使用 `vote_interaction mode=0`；Permission 不超过 20 个选项，ManualCheck 固定为安全失败优先的两个结果。支持的 Elicitation 单题单选使用 `vote_interaction mode=0`，单题多选使用 `vote_interaction mode=1`，2-3 个单选题使用 `multiple_interaction`。`task_id` 使用 `<delivery_id>@<request_nonce>`：复用每次发送的请求 ID 派生 32 hex nonce，生产 ID 共 97 字节，满足同机器人每次创建唯一、允许字符和 128 字节上限；不新增卡片业务 identity 或持久字段。button `key` 只允许 `<task_id>:action_index`，vote/multiple 的 `option.id` 只允许本地短 index，submit key 只允许 `<task_id>:submit`，均不得携带 action JSON、签名 token 或其它安全状态。`sub_title_text` 的 112 字是官方建议而非协议硬上限；追问详情必须完整展示 typed 问题和固定选项，markdown 与卡片 payload 超 32 KiB 时只允许在构造层截断展示字符串，禁止截断序列化 JSON 或任何 identity 字段。

为消除 Permission 卡按钮横排截断，生产 presentation adapter 仅把 Permission 从 `button_interaction` 改为官方 `vote_interaction`：选项来自当前 pending permission 投影后的 allowed actions，`checkbox.mode=0`，`option.id` 只使用本地 action index 短引用，提交按钮使用 `<task_id>:submit`，`task_id` 按上述请求 nonce 规则生成。中文卡壳固定为 `权限审批（id=xxxx）/ Agent 请求执行命令`；`xxxx` 是从 durable outbox rowid 派生并补零到 4 位的 `display_ref`，取值为 `rowid % 10000`。它只用于用户把 markdown 详情与 vote 卡配对，不新增卡片业务 identity，不写入 expected state，也不得被回调解析或反查。同一 delivery 的发送顺序固定为“markdown 详情帧 -> 等待 ACK -> vote 卡帧 -> 等待卡片 ACK”；重试与重启后同一 rowid 保持同一编号，最近 10,000 次 outbox 插入内不会重复；只有卡片 ACK 成功才把 delivery 标记为 sent，详情帧没有独立 outbox 行或业务 identity。旧 wire 标识下的选择回传协议闸门曾用真实企微账号完成：手机端选择 `option.id="0"`、PC 端选择 `option.id="1"`，真实回调均在嵌套 `template_card_event` 中返回 `selected_items.selected_item[].option_ids.option_id[]`，并能唯一还原出站选项；更新卡省略 `submit_button` 会触发平台码 42049，因此终态必须保留原 submit key。2026-10-16 新 wire 标识的真实点击验收状态见 §15。回调若只返回 submit key、缺少选择、多选、question key 不匹配或 task 不一致，均不得用默认选项、点击顺序或展示文案猜测授权语义。

Elicitation 使用 §7.2 的 `RemoteElicitationForm { questions, decline_allowed }`，每道 `RemoteQuestion` 复用领域 `ElicitationQuestion`，并携带约束与 `custom_answer_key`。领域模型不限制题数；每个渠道经 `fit_form` 得到实际发布的表单，并随 `ElicitationFixedForm { form }` 保存到该 delivery。企微把这份表单渲染为 vote/multiple 控件，回调转换成 `ImSubmittedAction::ElicitationForm { answers }`。selector key 按题目顺序生成 `q0`、`q1`……，可投递数量由渠道能力决定；option id 是选项序号的规范十进制写法。

connector 与通用入站层共同校验 msgid、私聊 actor/conversation、delivery/task、submit key、question key、option id、required selector、重复选项和非法 index；领域函数 `remote_form_content` 按 outbox 表单中的 typed scalar value 构建 content，再由 canonical 提交校验原始 `requestedSchema`。空多选按未作答处理，content 省略该字段，不写入空数组；必答题因此被拒绝，非必答题仍须通过原始 schema 校验。缺失选择字段能否解码还受各渠道 wire 契约约束，不能把飞书的缺失字段规则推断成企微协议。`timelineIdentity` 仍不参与 expected state 指纹。

企微表单的验收证据与仍未关闭的项目统一见[飞书技术方案 §17.1](../../开发计划/IM接入/飞书远程干预技术方案.md#171-当前验收状态)：2026-10-15 P2 确认覆盖三题正常投递、四题整单跳过等记录中的场景，不据此自动关闭更早的其他真实平台待测项。

Elicitation markdown 详情除工作空间、任务、节点、当前 message 和题目选项外，还携带当前 root 分支最新可见 `textDelta` 的“前序模型输出”快照，用于解释“是否确认上述方案/分支”这类依赖前文的问题。读取必须复用 timeline index 并按 4096 字符有界，不读取完整历史、hidden thought、嵌套 Agent 分支或用户私密输入；缺失时不猜测。若前序输出、message 或 question description 规范化后相同，只渲染一次。

权限卡片的结构化事实只能从当前 `PendingPermissionState.params` 投影：`rawInput.command + args` 合成完整 `permissionCommand`，存在命令时不再把 command/args 降级为通用参数；其余请求保留 `permissionTool`、`permissionPath`、`permissionParameter`，不再把 `permissionTitle` 降级为横向字段。`rawInput.cwd` 在没有 locations/path 时投影为执行路径。企微详情消息完整展示工作空间、任务、节点、说明、工具、命令、路径与参数，正文和字段仍按 256 字符有界；vote 卡横向字段只按“工具、命令、路径、参数、其他结构化字段”输出且遵守平台 26 字建议，不重复工作空间、任务、节点，超长事实由上一条详情承担。原始 `_meta.permission.title/description` 只留在 Runtime 提示语义中，不控制企微 Permission 卡壳。

ManualCheck 详情先读取当前 attempt ACP timeline 的最新可见 root `textDelta`，并按 4096 字符有界快照作为 `InterventionPrompt.message`；读取走既有 timeline index，只按排序候选读取命中的事件，不加载完整历史，也不把嵌套 Agent 分支、thought、空 chunk 或 hidden 输出当作当前节点结果。若暂停等待人工检查时缺失可见模型输出，投影返回稳定状态错误，不用上一 attempt、artifact 或相邻消息猜测。企微详情消息标题为“人工检查（id=xxxx）”，正文展示工作空间、任务、节点和“最后一轮模型输出”；vote 卡只使用同一标题并按“成功、失败”展示，成功默认选中，不重复上下文字段。提交仍通过原 allowed action index 调用 `submit_manual_check`，不得用展示文案、展示位置或默认选项推断结果。

桌面按钮与 IM 提交 ManualCheck 时必须收敛到同一条桌面应用执行边界：先按 outbox/桌面 snapshot 校验 expected state 并获取 `ManualCheckSubmissionLease`，再执行 scheduled attention resume，把 resumed occurrence 写入本次 App context，配置与桌面续跑一致的 ACP live/session/prompt lifecycle callbacks，最后调用 `submit_manual_check_background` 并等待 Runtime launch ack。IM inbound 不得直接调用 Tauri command、绕过 scheduled resume 或启动缺少前端回调的 headless 续跑；两条入口只能有一个 first writer，重复提交仍由 lease 与 inbound 幂等收敛。

桌面按钮与 IM 提交 Permission 也必须收敛到同一条应用执行边界：expected state 校验与权限响应写入使用共享 `InterventionCommandService::execute`；随后统一执行 scheduled attention resume、记录 PermissionResolved metrics resume cause、重建 ACP session/dynamic session 投影、向前端 emit session update 并触发 attempt 索引。IM 不得只写权限响应而不清理前端 pending permission 投影；重复合法回调返回 `AlreadyApplied` 时仍要发出最新 session 投影。

桌面按钮与 IM 提交 Elicitation 也必须收敛到同一条应用执行边界：先 inspect 并校验 expected state，在 response signal 可见前 reclaim scheduled interaction，记录 ElicitationResolved metrics resume cause，再通过共享 `InterventionCommandService::execute` 写 response signal，随后重建 root/dynamic ACP session 投影、emit session update 清理桌面对应的 `pendingInteractions` 并触发 attempt 索引。Permission 与 Elicitation 共用 `interactionId + turnId + promptEventId` 持久身份，展示事件 ID 不得反向推导业务 request identity。前端收到同 session 的更高 `generation / coveredRevision / newestRevision / newestSeq` 投影时，空 `pendingInteractions` 是权威终态；即使有界 events 页未携带 response event，也不得用旧 pending 覆盖。只有未前进的陈旧 active snapshot 才可保留 live pending。不得为 Elicitation 手动伪造 workflow 续跑；自然续跑仍由 ACP waiter 消费 response、发送 JSON-RPC response 并清理 signal。桌面先处理、IM 后点击时按 AlreadyApplied 收敛但仍 emit 最新投影；并发双入口 first-writer-wins。

权限选项以 ACP `PermissionOption.kind` 为 canonical 语义，提交仍返回原始 `optionId`：`allow_once/allow` 显示“仅允许一次”，`allow_always` 与 `allow_for_session` 显示“本次会话允许”，`reject_once/reject/cancel` 显示“拒绝”，`reject_always` 显示“始终拒绝”。Claude 退出计划模式的 `auto/acceptEdits/bypassPermissions/default/plan` 继续使用 qualifier 映射为“自动模式 / 自动编辑 / 跳过权限 / 手动确认 / 保持计划”。企微标准 Permission vote 按“本次会话允许、仅允许一次、拒绝”展示并默认选中第一项；展示顺序只作用于 presentation，option id 继续携带原 allowed action index。安全拒绝/保持计划仍作为超 20 项时的有界降级候选，不能因展示顺序调整而改变 canonical action 或授权结果。允许类重复且无法区分、未知 kind 或超过控件能力时，完整展示请求但只保留可确认的安全动作或引导桌面端处理。官方 option 无说明字段，不得把说明塞入 `text` 或用展示文案反查语义。

三类干预的详情上下文必须统一投影为保留字段 `workspaceLabel`、`taskTitle`、`nodeLabel`，在企微中只渲染于独立 Markdown 详情，不进入企微交互卡片；飞书卡片内按 id、节点、任务、工作空间顺序展示。工作空间优先使用当前 workspace 的 canonical display name，缺失时回退规范化目录名；任务优先使用事件标题或当前 Task 标题，缺失时回退 `task_id`；节点在工作流中必须是当前角色名称，在 Direct 中是 Agent 展示名，在 AUTO 中必须是当前动态角色 title。工作流节点不得用 Provider 展示名替代角色名称。

`askUserQuestion`/elicitation 的 message、问题标题、描述、单选/多选类型和 typed 选项只能从当前 `PendingElicitationState.request.requestedSchema` 结构化投影。详情消息先展示工作空间、任务、节点，再展示前序模型输出和问题内容；交互卡只展示问题控件，不重复上下文。企业微信的远程范围：单题 scalar 单选、单题 scalar 多选、2-3 个 scalar 单选字段可发送 IM；自由文本、复合值、混合多选、四题及以上、超过控件容量、选项值超界或移除 custom companion 后无法通过原始 schema 校验的请求一律在入队前跳过，不产生 outbox 行、markdown、提示卡、死信或重试，桌面端原生处理不受影响。带 `_askUserQuestionCustomAnswer.isCustomAnswer` 的 companion field 不作为独立问题展示，也不进入 IM 表单；单题单选带 custom answer 时 IM 仍只提交固定 scalar，自定义答案在桌面端填写；2-3 个单选题无论是否存在 custom companion，分类与提交均只基于主问题。企微卡不提供通用 Decline 按钮；飞书与桌面端可拒绝回答。

模板卡片事件只接受官方 `cmd=aibot_event_callback` 且 `body.event.eventtype=template_card_event` 的一对一私聊结构。真实运行时把业务 payload 放在 `body.event.template_card_event` 中，`event_key`、`task_id` 与 vote/multiple 选择必须从该嵌套对象读取；官方 SDK 1.0.7 类型中的平铺字段形状不代表平台实际回调。官方回调中的 `chattype` 可选：显式 `single/private` 为私聊；缺失 `chattype` 且缺失群聊 `chatid` 时按私聊候选处理，以 `from.userid` 作为 conversation/destination identity；显式群聊或存在群聊 `chatid` 时拒绝。`body.msgid` 是平台单次回调幂等 identity；同一 canonical intervention event 后续点击可能携带新的 msgid，Gold Band 必须用入站审计表的 `channel + canonical_event_id` 索引将其收敛为 `ALREADY_APPLIED`。opaque `headers.req_id` 只在内存中随 `ImActionResponseContext` 传递。button 先解析短 `event_key` 取得完整 wire task、delivery 前缀与 action index，显式 task 不一致即拒绝，缺失时用该完整 wire task 更新原卡；vote/multiple 先验证 submit key 与必填 task 一致，再解析 question key 与 option id。Permission 的 question key 为 `permission_choice` 且只能返回唯一 option；Elicitation 转换为泛型 Form selection 后由 outbox 的 `RemoteElicitationForm` 还原原始 scalar value。不得用 `req_id` 替代 callback identity，不得生成新 `req_id`，也不得因发送 ACK 没有 `msgid` 而跳过点击响应。解析失败只记录稳定 `parse_reason` 与字段存在性；若私聊回调仍可定位原卡，先尝试把原卡更新为失败态。嵌套 `eventtype=disconnected_event` 进入稳定 `IM_CONNECTION_CONFLICT`，不持续重连争抢。

历史 delivery 中的 destination 只表示发送时接收人，不是后续动作的持续授权事实。每次可操作回调在进入 `InterventionCommandService` 前，必须在 IM settings 写边界内重新读取当前 durable binding，并严格比较 channel、destination、conversation 与 authorized actor；当前 binding 缺失或任一字段变化时返回 `IM_BINDING_REVOKED`，不得 inspect、resume 或修改 Runtime canonical pending state。解绑持久化是授权撤销的线性化点：此前已通过 admission 的动作允许完成，此后进入 admission 的旧卡动作必须拒绝；settings 锁只覆盖读取与比较，不跨 Runtime 执行或网络响应持有。平台已展示的历史内容无法召回，撤销契约只保证旧卡不再执行。

`InterventionCommandService` 的 expected state 校验保持 first-writer-wins，但指纹输入必须与数据所有权一致：pending permission/elicitation 文件中的 `timelineIdentity` 只是展示索引回写，不改变用户待决策的问题、选项、expiry 或 canonical request。指纹序列化显式排除该字段；修改 params/request schema 或允许动作仍必须产生 `INTERVENTION_REVISION_CONFLICT`。

企微 Permission 终态更新帧必须与已验证 POC 保持同形：`main_title.title` 使用“id=xxxx 已处理：<动作>”，`main_title.desc` 为 `Agent 请求执行命令`；同时保持 `vote_interaction`、原 `task_id`、原 `submit_button.key`、`checkbox.disable=true` 和最终选中项 `is_checked=true`。ManualCheck 终态标题同样使用“id=xxxx 已处理：<检查结果>”，并保持禁用 vote、原 task/submit key 和最终选中项，描述使用人工检查摘要。Elicitation 终态必须保持原 vote/multiple 卡：vote `mode=0/1` 均设置 `checkbox.disable=true`，提交项 `is_checked=true`；multiple 每个 `select_list[].disable=true` 并把 `selected_id` 指向最终选择。官方更新协议没有 `submit_button.disable` 字段，Gold Band 不得发送该无效字段；即使客户端仍允许点击提交按钮，重复的新 msgid 也必须在 Gold Band 按 `channel + canonical intervention event` 收敛为 `ALREADY_APPLIED`，不得再次执行 Runtime 或把卡片更新为失败。终态标题把 `display_ref` 放在最前，摘要可截断但编号不可丢失；已结算请求的 AlreadyHandled/RevisionConflict 收敛为已处理，请求仍待处理的 RevisionConflict（卡片快照已过期）收敛为已过期，业务失败更新失败态但仍保持原结构。若平台拒绝更新 ACK，只记录是否存在 `cmd`、顶层/body 字段名、数字错误码层级和稳定错误码，不记录用户、凭据、文案或完整回调 JSON。

桌面 Runtime 在 blocking 边界返回通用 `ProcessedInboundAction { delivery, handling, settled_resolution }`。不在通用层保留企微专用类型；平台回包上下文由 connector 持有和解释。每个 InboundAction 独立任务处理，runtime 长期持有同一个 `ImInboundActionService` 和 ActionLocks，重复同一事件只执行一次。

桌面三类 command 在提交前捕获稳定源事件，提交后保留领域返回状态，只有 Accepted 才进入统一终态投影；AlreadyApplied 仍沿用 session 刷新，但不广播或改写处理来源。IM Accepted 同样通过该终态入口投影。通知矩阵与 tombstone 语义见 §6.4。

### 6.2 微信延期门槛

微信不进入首期实现。只有使用真实账号和官方协议完成“客户端未先收到用户消息时仍能稳定主动推送”、休眠恢复、context token 生命周期和长期重连验收后，才可重新评审范围；不得交付需要用户先发送“待处理”的 pull-only 版本。

### 6.3 飞书接入与卡片终态

两渠道扫码会话均持续到配置提交完成，不能在平台授权成功时提前结束。凭据库写入期间仍可取消或替换会话；提交配置时以当前 session ID 为准，与取消/替换互斥。取消/替换先完成则拒绝旧提交并删除本次暂存凭据，保留已有配置、凭据和新会话；配置提交先完成则保留已提交结果，后续关闭不撤销配置。网络与凭据库操作不持有会话/设置锁。

1. 在设置页点击扫码接入，先确认“发送消息、读取发给机器人的私聊消息、更新机器人卡片、读取你的姓名”四项权限，再发起扫码。取消或关闭会话使迟到结果失效。首次接入和已配置后的“更换飞书应用”都使用飞书原生选择页，可选择已有应用或新建应用。扫码 addons 在三项必需 IM 权限之外追加 `contact:contact.base:readonly`（通讯录 API 基础权限）与 `contact:user.base:readonly`（返回 `name` 的字段权限）；这两项不进入自检的必需权限，缺失不触发重新授权。
2. 后端轮询授权，把 app_secret、扫码人 open_id、brand 写入 keyring；前端只收到 sessionId、qrUrl、expiresAtMs 和非敏感设置。新建应用的预填名称取自渠道产品名（`wb` 为 `MALING`，`default` 为 `Gold Band`）；选择或新建模式不发送 `createOnly`，重新授权当前应用才发送已有 app_id 作为 `clientID`。
3. 授权后，扫码账号向机器人发送一条私聊消息完成绑定；持久 binding 与当前 generation snapshot 一致后才显示“可用”。其他账号不能抢占绑定。
4. 接收账号姓名：connector 每次 `connect` 在 bot 自检后用 tenant token 调用一次 `GET /open-apis/contact/v3/users/{open_id}?user_id_type=open_id` 读取扫码账号 `name`，随该连接上报的 `BindingObserved.display_name` 持久到 `ImBindingSummary.display_name`（`Option`）。不按消息查询；查询失败（缺权限、不在通讯录范围 41050、网络）只记 warn，绑定照常完成，设置页显示中性文案“已绑定一对一私聊”，不再展示 open_id。连接成功（`Connected.owner` 携带已解析的 owner open_id 与姓名）后，runtime 在阻塞线程池内对同一 owner 的已有绑定立即采纳该姓名（例如替换旧版写入的 open_id），仅在姓名变化时写设置并刷新快照，无需等待下一条消息；查询失败时 `owner` 为空，不会抹掉已知姓名。企微保持原样（userid 即展示名）。
5. 支持启停、六类通知偏好、重新授权当前应用、更换飞书应用、真实重连和删除。更换飞书应用完成后替换当前应用并清除旧私聊绑定；重新授权只针对当前应用。重新绑定只清除私聊绑定，仍由扫码账号发消息绑定。缺权限引导重新授权，不无效重连。
6. 页面显示 App ID、运行期机器人名称和单设备提醒。Secret、扫码 ticket、access_key 不进入前端或日志。

- 飞书卡片初态与终态均按 `id → 节点 → 任务 → 工作空间 → 题目` 展示。提交成功后保留原表单结构，单选、多选、自定义回答和文本回填 Accepted 的答案，控件禁用，展示处理来源。
- 终态投递先读取源 outbox 生命周期：源消息仍在 pending / sending（包括失败重试、租约恢复）时，终态留在现有 outbox 延期，不能把尚未取得消息 ID 当成永久不可更新。源发送结算并持久化绑定后更新原卡；源已过期、永久失败或结算后无绑定时，才允许另发只读终态。
- 源消息发送失败重试与租约回收只在同一 SQL 语句内确认该目标尚无 `:resolved` 终态时才回到 pending；终态已存在时源 delivery 直接转为 expired，已结算的源卡片不得被重发。
- 即时回调、后台 PATCH、超时后的补写和不可更新后的另发消息使用同一份结构化终态结果。不得让后续 PATCH 清空回调已显示的答案；迟到或失败的提交不得显示成赢家答案。
- Runtime 仍是唯一业务事实源。终态 outbox 的 `ElicitationSubmitted { content }` 只保存已接受答案的投递快照（最大 8 KiB，随 outbox 7 天保留策略清理）；回包不再有独立瞬态 submission。答案不写日志，Debug 脱敏。旧终态没有保存答案时明确显示结果详情不可用。
- payload v3 / IM schema v5 的一次性迁移保留源卡片、消息绑定、rowid/display_ref、终态及 tombstone；旧无答案终态转成 `ResultUnavailable`，不伪造空回答。新读取路径不兼容旧格式。处理来源仍仅由获得 Accepted 的一端写入。
- 企微原投票卡/多题卡回调更新和跨端另发文本规则保持；本次授权的能力修正仅将多题选择器上限从 5 改为官方的 3。超过上限不拆卡、不分批提交，飞书和桌面端处理路径照常。
- 企微 `Handled / AlreadyHandled` 原卡更新只从已接受的 `outcome` 恢复权限、人工检查及追问答案；回调携带的本次选择不能代替赢家结果。选项以稳定 option ID 或 schema 中的原始值匹配原 delivery；结果不可用、不匹配、拒绝回答或字段未回答时不勾选、不默认选择第一项。失败/过期回调保留原错误显示，不把它当作已接受结果。

### 6.4 三端终态通知矩阵

| Accepted 来源 | 桌面端 | 企业微信 | 飞书 |
|---|---|---|---|
| 桌面端 | 本地生效并刷新会话 | 另发同编号“已在桌面端处理”文本 | PATCH 原卡，结果和桌面来源 |
| 企业微信 | 共享应用边界刷新 session | 5 秒回调更新原卡，保留选择 | PATCH 原卡，结果和企微来源 |
| 飞书 | 共享应用边界刷新 session | 另发同编号“已在飞书处理”文本 | 回调终态 + 相同结果的幂等 PATCH |

- 终态投影只处理当前 eligible target（启用、有凭据/私聊绑定、具备投递/卡片能力且启用对应通知）。已有源 delivery 的目标写 Send/pending；没有源 delivery 的目标只写 TombstoneOnly/expired，拦截迟到源投影，不发送、不唤醒 worker、不计 active 配额。
- 企微是来源时仅回调更新，不重复广播到自身：被点击的企微目标写 TombstoneOnly/expired 保存赢家结果并关闭源卡，不另发第二条；飞书来源仍写终态，确保超时和 generation 切换后可由 worker 补齐。
- Runtime 停止路径（Run 停止/节点 teardown、ACP 会话关闭、Direct 后台停止、Runtime 已消失的遗留请求）不经过桌面命令，统一由 canonical settlement 收敛：领域 `InterventionCommandService::settlement` 依据响应文件 → 待处理文件 → timeline 已结算项判定 `Pending | Answered | Cancelled`（无信号文件但 timeline 仍待处理视为被遗弃的取消）。IM 侧 settlement reconciler 在 RunCompleted / NodeCompleted / AcpTurnFinished / DirectTurnLifecycle 后（1 秒去抖）及每 30 秒扫描未结算的开放源卡（pending/sending/sent、未过期、无 `:resolved`），只为 `Cancelled` 写 `resolved_by=Runtime` 的取消终态；`Answered` 留给 Accepted 的一端写入，保持单赢家规则，重复扫描靠终态唯一键幂等。
- 点击已结算请求的卡片时：已有终态直接回填；canonical 为取消则当场投影取消终态并回填；答案仍在写入途中则回“已处理”并禁用原卡，等待后续 PATCH 补齐结果；请求仍待处理（卡片快照过期，永远无法成功）则按“操作已过期”禁用原卡。领域对已结算请求返回 `INTERVENTION_ALREADY_HANDLED`，不再误报 `INTERVENTION_REQUEST_NOT_FOUND`。
- 回调最多等待 2.5 秒，超时只返回“处理中”toast，原输入不被清除。Accepted 后的持久终态继续 PATCH；被拒绝时卡片保持可编辑，原因另发一条只读消息“id=xxxx 提交未成功”到同一私聊（拒绝没有持久终态，不进 outbox，也不改卡片，避免迟到的可编辑卡覆盖已结算终态）。
- 只有消息撤回、删除或超过更新期限的 `MessageUpdateUnavailable` 才另发源上下文完整的只读终态卡；限流/网络错误退避重试，鉴权/缺权限/卡片协议失败不另发掩盖错误。
- 两端竞争只允许一个 Accepted。迟到提交从已有终态读取赢家答案；没有可读取的赢家结果时回“已处理”并禁用原卡，不使用后到答案填卡。领域 Accepted 与 IM 写入间崩溃的恢复限制保留，不新增跨库事务。

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
| App Secret、扫码人、brand | Credential store | 操作系统凭据库 |
| 飞书 tenant_access_token | Connector | 单实例内存缓存，不落盘 |
| 系统原生通知展示与点击去重 | Desktop notification | 现有 `InterventionNotification` / `NotificationDedup` |

IM 数据库不得保存 `approved/failed/completed` 作为工作流事实。它只保存动作请求、提交结果和投递审计；展示最终状态时重新读取领域结果或使用领域提交返回值。

### 7.2 核心模型

当前代码真源为 `src/app/intervention.rs`、`src/im/model.rs`、`src/im/connector.rs`，不保留旧布尔能力和旧表单变体：

- `ImChannelKind { WeCom, Feishu }`。
- `ImChannelCapabilities { proactive_delivery, card_actions, private_chat, message_update, interaction }`；`message_update` 为 `None | CallbackOnly | Anytime`。
- `ImInteractionCapabilities` 包含 permission_max_options、max_questions、单题/多题单选容量、multi_choice、free_text、custom_answer、decline。
- `RemoteElicitationForm { questions: Vec<RemoteQuestion>, decline_allowed }`；RemoteQuestion 包含 selector_key、领域 ElicitationQuestion、constraints 与 custom_answer_key。文本、多选、“其他”以同一领域 content 构建函数提交 schema-shaped JSON；反向由 `remote_question_answer` 把已接受的 content 逐题解码为 `RemoteElicitationAnswer`（选项 id、“其他”文本或文本），飞书禁用控件回填、“提交内容”行与企微终态标题都只消费这一投影（`connectors::accepted_answers`），按发布副本解码失败的题按未回答展示。发布副本里的固定表单统一经 `InterventionAllowedAction::fixed_form` 查找。
- `InterventionRef { locator, request, expected_state, allowed_actions, expires_at_ms }`；request 区分 Permission request_id、Elicitation elicitation_id、ManualCheck。locator 包含 project/task/run/round/node/attempt 与可选 outer locator。
- `ImSubmittedAction` 为 PublishedAction、PermissionOption、ManualCheck、ElicitationForm、ElicitationDecline；平台类型不越过 connector。
- `ImDeliveryPayload` 为带 version 的 Intervention、Information、InterventionResolution。只有 Intervention 可产生可执行入站；终态仅用于投递，不可再次提交。
- `InterventionResolutionNotification` 保存 source_event_id、canonical_event_id、notification_kind、locator、display_ref、resolved_by、outcome。
- `ImResolvedBy { Desktop, Channel(ImChannelKind), Runtime }`，`Runtime` 只标记由 canonical settlement 观察到的取消；`ImResolutionOutcome` 保存权限 option_id、已提交答案 content、拒绝回答（ElicitationDeclined）、请求未获回答即结束的取消（Cancelled，适用权限、追问与人工检查）、人工检查结果或显式 ResultUnavailable。
- 只有用户真实拒绝才是 Declined；领域 `ElicitationAction::Cancel` 与 settlement 的 `Cancelled` 映射为 Cancelled，只能由桌面/Runtime 的停止、超时与孤儿清理路径产生。`ImSubmittedAction` 没有取消变体，已发布的 allowed actions 也不包含取消，远程入站无法提交 Cancel。取消终态不归属任何处理端：飞书卡片标题和结果均显示“已取消”且不显示处理端，企微另发“id=xxxx 已取消”，不得显示为“拒绝”或“已在某端处理”。
- Cancelled（wire `cancelled`）与 `ImResolvedBy::Runtime` 是 payload v3 内的追加 serde 变体，不升级 payload/schema 版本：旧数据不可能包含它们，已有变体的编码不变，无需迁移；按开发阶段策略不支持新库回退旧客户端。
- `ImResolutionDispatch { Send, TombstoneOnly }` 分离持久化事实与发送副作用。终态事件 identity 为 `<source_event_id>:resolved`，delivery identity 为 channel + destination + notification_kind + canonical_event_id 的稳定哈希。
- `ImConnectionState` 为 Disabled、Connecting、Reconnecting、Connected、AuthenticationRequired、Error；快照携带 generation、capabilities、identity、binding、last_error_code。

每个 eligible channel/destination 分别保存一份实际发布的表单；入站据此校验而非用另一个渠道的能力猜测。action_id 来自 channel + platform_event_id，display_ref 只用于人类配对，不参与授权或请求身份。

payload v3 / IM schema v5：v3→v4 在 IMMEDIATE 事务内删除旧 v1 干预及旧 desktop-resolved 终态，FK ON 级联删除消息绑定、保留入站审计；v4→v5 原位转换 v2 JSON 并保留 rowid/绑定/tombstone，旧无答案终态改为 ResultUnavailable。失败整体回滚；不保留旧格式运行期兼容层。SQLite 的 payload_kind 仍将终态归为 information，typed JSON 区分三种 payload。

带 scheduled_occurrence_id 的 Run/ACP 信息事件不投递 IM；定时任务的 Permission、Elicitation、ManualCheck 仍是干预事件。IM 不消费 Windows 通知 DTO、dismiss 或 NotificationDedup。

### 7.3 状态转换不变量

1. 每个动作提交前必须按完整 locator 读取当前 Runtime 状态。
2. `expected_state` 不匹配时拒绝写入，并返回“状态已变化”。
3. 同一 `action_id` 重放返回首次结果，不重复调用领域命令。
4. 同一 intervention 的首个合法终局决策生效，后续竞争动作返回“已处理”。
5. 权限请求只能调用权限响应接口；追问只能调用 elicitation 接口；人工成功/失败只能调用人工检查接口。
6. `manual_check` 只在当前 attempt 为 `manual_check_pending` 且 locator 仍为当前 owner 时接受。
7. 连接 generation 单调递增；旧连接迟到的回调、断线和重连结果不得覆盖新连接状态。
8. connector 内部可发送句柄也由 generation 所有；只有同 generation 可安装或清除 sender，旧会话退出不得清除新会话 sender。
9. `sending` 是有期限的 claim，不是终态；worker settlement 必须携带 claim 时的 `attempt_count` revision，租约恢复后的迟到成功/失败不得覆盖新 claim。
10. 删除配置以 settings 中“不再存在该 channel”为 canonical 事实；跨 settings、投影队列、SQLite 与 keyring 的后续清理由 durable operation journal 推进，失败返回 pending 并由后台重试，不把旧配置复活。
11. ManualCheck 的 canonical 可执行条件只来自当前 Run/Node/Attempt 状态；Timeline 最新输出只是远程通知的可选展示投影，缺失或为空不得阻止桌面端或 IM 提交人工结果。

## 8. 设置与连接管理

设置页“通用”页新增“IM 远程干预与通知”区域，按接入生命周期逐渠道展示企业微信和飞书状态。未配置凭据时只提供“扫码接入”；凭据存在后，总开关、状态引导和六项通知始终可见。连接在线但尚无 canonical 私聊 binding 时派生显示“等待绑定”；只有 durable binding 已落盘、delivery target 已重建且当前 generation snapshot 携带同一 binding 时才显示“可用”。该展示模型不落盘。两渠道都只提供扫码接入/重新授权；飞书额外提供“更换飞书应用”，复用飞书原生已有应用列表。不保留 Bot ID/Secret 手工输入，Secret 不进入前端状态。

设置页 IM 区块使用与定时任务运行设置相同的前端 stale-while-revalidate 投影：App 启动预取、模块级缓存命中后立即渲染，已有数据时刷新不得退回「加载中…」。该缓存不是第二事实源；保存、启停、扫码、删除仍以后端返回的最新 VM 为准，connection snapshot 按 generation 单调合入。

Settings schema v12 在严格反序列化前一次性删除 v11 channel notifications 中的 scheduled completion、failure、attention、missed 四个废弃键，并通过现有设置写入边界原子回写。迁移后仍以六字段 `ImNotificationPreferences` 为唯一持久模型；当前版本重新出现废弃键必须报错，禁止恢复兼容读取或旁路消费。

设置区域在正常宽度和窄窗口下保持单列可滚动，平台字段与通知开关允许换行但不得横向溢出；中英文页签使用能在等分 segmented control 中完整呈现的短标签。键盘必须可切换设置页签和所有开关，保存失败通过 `role=alert` 展示 i18n 文案。

### 8.1 设置界面清晰度优化（2026-09-04）

- 根因判断：credential、enabled、connection generation、private binding 与 notifications 的权威状态设计正确；现有界面把不同生命周期阶段平铺成一个表单，属于正确设计下的展示和操作层级不完整，不需要新增 IM 状态机。
- 未接入时只展示“扫码接入”主操作；扫码授权后进入“发送一条私聊”步骤，收到 canonical private binding 后才进入可用态。接入进度完全由既有字段派生，不持久化 UI step。
- 页面顶部只给出一个用户结论：未接入、等待绑定、连接中、正在重连、可用、已暂停、需要重新授权或连接被占用；错误态在原位置给出对应恢复动作，不增加会伪造成功语义的本地测试状态。
- 六类通知仍保持原持久模型，但展示分为“需要你处理”和“执行结果提醒”两组；对客文案不暴露 Run、ACP、安装级目标、协议地址或凭据实现。企业微信标题行不展示实现注释，未接入说明只保留“尚未接入企业微信。接入后可设置远程干预与通知。”
- 只有未配置凭据时隐藏通知选项；等待绑定、连接中、暂停、网络错误、鉴权错误和连接冲突时仍允许编辑通知。Bot ID 使用安全断行，不展示敏感凭据。
- 总开关是即时运行命令，目标控件在 100ms 内进入处理中并以后端最新 VM 收敛；通知偏好使用独立本地草稿和窄保存接口，连接事件及启停响应不得清除 dirty 草稿。
- 重新授权、更换接收账号和删除配置进入更多菜单。更换账号与删除分别使用 AlertDialog：前者只清除 binding 和 delivery target，保留凭据、Bot identity、enabled 与通知偏好；后者删除配置、凭据和未发送数据。
后端命令按数据所有权拆分为 `set_im_channel_enabled(kind, enabled)`、`save_im_notification_preferences(kind, notifications)`、`reset_im_channel_binding(kind, expected_generation)`、`reconnect_im_channel(kind, expected_generation)` 与 `delete_im_channel(kind)`。旧通用保存和独立断开入口已经删除，不保留兼容层。snapshot 继续按 generation 单调合并；重连只启动真实新 generation，不提供测试连接。

`delete_im_channel` 先在同一 settings 写临界区创建 cleanup operation、持久化删除配置、移除投影 target，并立即推进 connection/connector generation；之后通过最长 30 秒的 projection barrier 确认旧 target job 已消费，再删除该 channel 的 active outbox 和旧 credential，最后完成 journal。后续清理失败或 barrier 超时时 command 返回已删除 settings、`operationId` 与 `cleanupStatus=pending`，启动阶段和运行期定时恢复按 phase 幂等续做；cleanup operation 存在时拒绝同 channel 重新授权，避免旧清理触碰新配置。

`BindingObserved` 不得直接进入公开 snapshot。connection manager 先校验 generation、私聊属性与既有 actor/conversation；设置写入与命令写入共享最小 IM settings 临界区。binding 可靠落盘并更新 DesktopState 后重建 delivery target，最后才提交同 generation snapshot。存储失败保持 connected-without-binding，并以 `IM_STORAGE_UNAVAILABLE` 启动每个 channel+generation 至多一个、最多三次、1/2/4 秒的可取消重试；generation 前进立即取消，旧结果不得提交。

恢复策略：网络与限流沿用 connector 的有界退避并显示正在重连；鉴权或凭据错误进入重新授权；连接冲突停止自动抢占，用户关闭其他客户端后发起一次真实重连；binding 存储失败保持等待绑定。所有用户文案由前端七语言 i18n 映射稳定错误码。

过度设计复核：不新增 aggregate、持久状态机、全局 store、轮询、队列或连接测试接口；接入步骤和顶部结论均由现有 canonical 字段派生。设置页展示缓存只复用定时任务已有的运行时 SWR 投影模式，容量固定为单份 `ImSettingsVm`，有新鲜期与 single-flight，不复制 canonical 数据。性能影响限于最多 2 个 channel、每渠道 6 个静态通知项、一次启动预取和常数级 DOM 投影；binding retry 有 generation、单任务、次数和延迟上限，不进入高频路径。

每个渠道的配置包含：

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
- 企微卡片按钮携带 `delivery_id:action_index` 短引用；飞书携带 deliveryId、actionIndex/表单选择与签名 actionToken。均不暴露本地文件路径、完整 locator 或应用凭据。
- action 短引用只允许还原本地 outbox 当前 `allowed_actions` 中的同索引动作，并继续校验 actor、conversation、delivery 生命周期和平台回调幂等。
- 用户回答正文设置明确上限，首期 8 KiB；超限在进入 Runtime 前拒绝。
- 删除平台配置时同步删除 OS 凭据、失效连接 generation，并清理该 channel 的未发送 outbox；审计记录按保留策略清理。

威胁模型不包含“已完全控制本机用户会话的攻击者”。本地数据库仍按 Gold Band 用户目录权限保护，敏感材料额外交给系统凭据库。

## 10. 离线、重试与容量

- 生命周期订阅者只执行 O(1) 转换与有界入队，不执行网络 I/O。
- IM 是可选 runtime。桌面 setup 只在首屏之外发起轻量激活判断：已有 enabled/credential/binding 配置或 `core.db` 中存在待恢复 channel cleanup 时，才创建 runtime、访问 keyring、构建目标并启动 delivery/maintenance/cleanup 周期任务；三者均不存在时不创建数据库、不注册 lifecycle subscriber、不启动周期任务。首次扫码或其它显式 IM 配置命令通过同一进程级初始化锁惰性创建并安装唯一 runtime，不复制状态或重复注册订阅者。
- lifecycle subscriber 必须先读取内存中的轻量 target snapshot；没有任何 enabled + credential + binding + notification 的可行动目标时立即返回，不读取 workspace state、不创建异步任务、不访问 SQLite。存在目标时仍按原 canonical event 顺序进入有界投影队列，实际 Timeline/Runtime/SQLite 投影继续在 blocking pool 执行。
- active outbox 默认上限 1,000 条；达到上限时先小批量清理已过期/终态记录，仍满则拒绝新的远程投递并记录结构化错误与桌面提醒，不得淘汰尚未处理的 active 干预，也不能无界增长。
- 未发送干预在领域请求过期后直接标记 `expired`，不得迟到投递为可操作消息。
- 企微网络错误使用带 jitter 的指数退避，1 秒起步、60 秒封顶并持续重试，直到连接成功、generation 被取消或出现鉴权失败、连接冲突、配置错误等永久错误；成功建立连接后重置退避。`ReconnectScheduled` 表示重试循环仍存活，`ConnectionFailed` 只表示终态，UI 不得根据错误码猜测二者。
- 飞书按 endpoint 下发的 ReconnectInterval + Nonce 重连；待回包表由 generation 所有，回包、超时、断连即清理，读任务不执行业务。
- 飞书 OpenAPI 发送与更新同属活动连接生命周期：使用连接 cancellation 的子令牌，断连清理、替换句柄和 generation 失效时取消旧句柄及其在途请求，包括等待/刷新 tenant token 的操作。取消返回现有可重试 `IM_NETWORK_UNAVAILABLE`，重试与删除后的清理由现有 outbox 承担；旧 generation 的迟到清理不得取消新连接。取消不能撤回平台已经接收的消息，不承诺远端事务回滚。
- 平台限流遵守 `Retry-After` 或 SDK 返回的重试时间；单 channel 串行化同一消息更新，其他消息可受控并发。
- `claim_due` 先用 due 索引只读确认当前 channel 是否存在到期或已过期 pending 行；确认完全为空时直接返回，不申请 SQLite writer lock。存在候选后才读取最多 32 条并在写事务前完成 typed 解析；有效行以 rowid、state、attempt count 做 CAS claim，单条损坏行原子转为 `dead_letter + IM_PAYLOAD_INVALID`，不得把同批健康行改为 sending 后再让整批解析失败。SQLite/锁/事务错误仍是批次级错误。
- 企业微信单 session 最多保留 64 条 pending request。调用端创建的 15 秒总 deadline 随 command 进入 session，由同一 session 使用 `DelayQueue` 在 ACK、发送失败、断连、取消或超时时删除；linked-detail 两帧共享同一 deadline，迟到 ACK 不得重新匹配，达到上限返回 retryable `IM_QUEUE_CAPACITY_EXCEEDED`。
- outbox 已发送/过期记录默认保留 7 天，入站动作幂等记录默认保留 30 天；周期清理采用有上限的小批次。
- 应用启动时先恢复过期发送租约并执行一次 retention；运行期每 30 秒恢复租约、每 6 小时清理 retention。单轮最多 4 批、每批最多 200 条，所有 SQLite 工作在 blocking pool 中执行。
- 应用正常退出时先关闭 Runtime admission gate，再停止 scheduler，随后通过独立 cancellation token 取消 IM worker、连接与长轮询；控制信号不得与有界 delivery data queue 共用可能阻塞的 `send`。delivery worker 必须能中断挂起的 connector send，并在 2 秒总预算内 join，超时任务直接 abort，不能无界延迟退出。

客户端退出、休眠或无网络时没有云端在线实体，这是无网关方案的明确产品限制。恢复在线后只补发仍然有效且尚未处理的干预。

## 11. 错误契约

后端仅返回结构化 `ImIntegrationError { code: ImErrorCode, retryable, retry_after, platform_code }`；code 序列化为稳定 IM_ 前缀值，不按字符串片段猜测平台错误，不回传原始错误文案。

- 连接：IM_CONFIG_INVALID、IM_CREDENTIAL_UNAVAILABLE、IM_AUTHENTICATION_REQUIRED、IM_NETWORK_UNAVAILABLE、IM_RATE_LIMITED、IM_CONNECTION_CONFLICT、IM_SCOPE_MISSING。
- 入站：IM_ACTOR_MISMATCH、IM_CONVERSATION_MISMATCH、IM_BINDING_REVOKED、IM_ACTION_INVALID、IM_ACTION_EXPIRED、IM_FORM_ANSWER_MISSING、IM_FORM_OPTION_INVALID、IM_FORM_CUSTOM_ANSWER_MISSING、IM_FORM_TEXT_TOO_LONG。
- 投递：IM_PROTOCOL_INVALID、IM_STORAGE_UNAVAILABLE、IM_QUEUE_CAPACITY_EXCEEDED、IM_MESSAGE_UPDATE_UNAVAILABLE。
- 扫码：IM_SCAN_CANCELLED、IM_SCAN_EXPIRED、IM_SCAN_DENIED、IM_SCAN_NETWORK_UNAVAILABLE、IM_SCAN_SESSION_INVALID 等；完整值以 ImErrorCode 与扫码错误类型为准。

设置页七语言 catalog 映射用户文案。平台出站 renderer 与回调回包的卡片文案来自 `src/im/locales/{zh-CN,zh-TW,en,ja-JP,ko-KR,pt-BR,es}.json` 七语言 keyed 资源表，编译期嵌入；`ImLocale` 就是桌面语言 `DesktopLanguage`，未设置时与桌面启动一致取系统语言，飞书回调回包使用接收点击的会话语言，不再各自兜底为中文或英文。列表分隔符、“已处理”前缀、未知权限选项的“桌面处理”等也属于资源表；zh-CN 是语义来源，单测校验七语言 key、`{ref}`/`{source}` 占位符和 Agent、ACP、Run 等保留词一致。缺权限/凭据失效引导重新授权，网络故障引导重连；更新不可用是原消息生命周期问题，不与鉴权或协议错误混为一类。

## 12. 验收标准

### 12.1 领域正确性

- 桌面端与 IM 对同一请求并发操作时只有首个合法动作生效。
- 过期、旧 revision、错误 actor、错误 locator 和重复平台事件均不能改变 Runtime。
- 人工成功/失败不能用于非 `manual_check_pending` attempt。
- IM 连接、数据库或平台故障不影响 Runtime 正常执行和桌面端本地干预。

### 12.2 平台行为

- 企业微信、飞书可扫码接入、绑定授权私聊、接收三类干预和启用的信息通知；飞书支持 §5 的完整表单、只读答案回显与 §6.4 三端终态矩阵。
- 自动化固定仅一个 Accepted、迟到提交不覆盖赢家、回调超时由 PATCH 补齐、重试/降级分类和 tombstone。真实账号测试与本地 mock 结果分别记录。
- 飞书扫码必须在四项权限确认后才发起 begin；缺权限可重新授权；七语言文案、窄屏、主题和扫码取消通过前端验证。
- 微信 connector 和设置入口保持缺席，直到稳定主动推送真实 PoC 通过。
- 客户端重启后恢复启用配置和未过期 outbox，但不复活旧连接状态或已处理动作。

### 12.3 安全与隐私

- 仓库、workspace、`settings.json`、`core.db`、日志和前端 state 中均无明文 Secret/token。
- 未绑定会话、错误 actor 以及不再匹配当前 durable binding 的历史卡动作被拒绝且不改变 Runtime。
- 解绑、更换接收账号或删除配置后，旧卡片不可继续操作；删除后凭据不可再读取。

### 12.4 性能

- lifecycle publish 路径不等待网络，入队为 O(1) 且有界。
- 零 IM 配置且无 cleanup 的桌面进程不访问 keyring、不打开或建表 `core.db` 的 IM schema，也不创建 5 秒 delivery、30 秒 lease recovery 或 60 秒 cleanup recovery 周期任务。
- 空 delivery poll 只执行一次命中索引的 `EXISTS` 只读查询，不进入 `BEGIN IMMEDIATE`，因此不与 scheduler、runtime recovery 或 workspace identity 争用 SQLite writer lock。
- lifecycle projection 的 blocking task、repository/validation 结果必须形成可观测 completion；只有持久化完成后才可唤醒 worker，单条失败不得终止后续投影。
- 正常运行每渠道最多一个 WebSocket，总计最多两个；跨渠道慢处理不得互相阻塞。
- 不扫描完整 timeline、全部会话或历史 run 来构造通知。
- 设置状态更新仅重渲染对应集成区域。

## 13. 方案审视

2026-10-15 当前方案复核：P3 补齐正确设计下的 Accepted 门禁与 PATCH 错误分类，P4 复用现有扫码、连接快照和 shadcn/ui；不新增表、缓存、队列或审批状态。终态查询按稳定 ID/既有索引，最多两个目标，每表单最多 5 题/8 KiB 答案；UI 只更新对应渠道，二维码按需加载，不触发会话历史刷新。P2 冷/热入站实测分别为 121ms/242ms，未据此增加缓存或预热。以下按日期保留历史根因记录，当时的范围与接口以本文当前 §4–§12 为准。


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

2026-09-17 三类干预详情上下文确认属于 linked-detail 公共投影契约缺失，而不是 Runtime、outbox 或审批状态机设计缺陷：原有 `InterventionPresentation.fields` 足以承载展示快照，但权限/追问 ACP 事件的节点在工作流场景错误回退为 Provider 展示名，且卡片渲染会自然消费任务/节点字段。修复统一生成 `workspaceLabel`、`taskTitle`、`nodeLabel`；标准工作流只允许 `profileName/profile/node_id` 解析角色，彻底删除 Provider fallback；AUTO 主路径读取当前节点的 `dynamic_node_file` 并使用其 title，仅在节点投影缺失、不可读或 title 为空时回退 `DynamicGraphState`。三个字段仅进入 Markdown 详情，卡片显式过滤，不新增第二套业务身份或投递状态。

2026-09-17 ManualCheck 生产暂停发布链路补齐确认属于正确领域设计下的 lifecycle 发布实现不完整：人工检查暂停只发布了通用 `RunPaused`，而 IM 投影只消费语义事件 `InterventionRequested`，导致审核详情与成功/失败 vote 卡无法进入 outbox，桌面处理后的通用终态确认也因找不到原 delivery 而无法发送。修复在 canonical 暂停状态持久化后，针对 `manual_check_pending` 复用同一 event ID 补发 `InterventionRequested { request: ManualCheck, kind: ManualDecisionRequired }`；不修改 connector、不增加结果字段或第二套审批状态。桌面来源仍只发送“已在桌面端处理”的通用确认，企微来源继续更新原卡。

2026-09-16 对现有桌面功能的性能与退出复核确认：问题不在 outbox、generation 或 Runtime canonical 设计，而在可选 IM 生命周期被无条件装配、空 claim 仍进入写事务、subscriber 在确认目标前读取 workspace state，以及 shutdown 把控制信号塞入有界 data queue。修复保留同一 `DesktopState` runtime slot、settings、cleanup journal 和 cancellation token，只补齐惰性激活、只读空队列闸门、target-first 快速返回与 gate → scheduler → IM 的退出顺序。ManualCheck 同时恢复原设计边界：Timeline 文本只用于远程展示，不是 canonical command 的 admission 条件。

2026-09-16 Elicitation 桌面提交复核确认：共享 `InterventionCommandService` 负责 pending identity、expected state、幂等与 first-writer-wins 是正确设计，但实现错误地把 `RemoteElicitationForm` 生成的 transport `allowed_actions` 同时用于 canonical Accept 准入，导致自由文本、自定义答案和超过三个问题等桌面可处理表单返回 `INTERVENTION_ACTION_INVALID`。修复后 canonical Elicitation 只按完整 ACP `requestedSchema` 校验对象答案或 Decline；`ImInboundActionService` 继续在调用 Runtime 前按 delivery 中实际发布的 `allowed_actions` 校验 token、固定表单选择与动作，`requires_desktop` 只描述投影能力，不反向限制桌面命令。

### 13.2 过度设计评审

首期不引入云端网关、Kafka、通用事件平台、独立 helper 进程、新审批 aggregate 或自然语言 Agent。新增的连接管理器、outbox 和幂等表分别对应长连接生命周期、进程离线重试和外部事件至少一次投递三个真实不变量，复杂度与风险相匹配。

2026-09-04 可靠性闭环不新增通用 scheduler、第二 outbox、配置 revision、租约表或前端 canonical 副本。现有 `state + next_attempt_at_ms + attempt_count` 已能表达 claim lease，现有 connection generation 已能表达 sender ownership，现有 `AcpUiEvent.id` 已是 occurrence identity；只新增 cleanup journal，因为 settings、SQLite 与系统凭据库无法共享事务，而删除必须具备可恢复进度。

2026-09-07 修复不新增 binding epoch、撤销表、outbox 状态、数据库迁移、重连 supervisor 或外部依赖。现有 durable binding 足以表达当前授权，现有 `dead_letter + last_error_code` 足以隔离坏行，已有 `tokio-util::DelayQueue` 足以统一 pending deadline；`Reconnecting` 只是连接管理器的 transient lifecycle 状态，不形成第二份持久事实。

2026-09-16 修复只增加一个进程内初始化互斥，防止启动恢复与首次配置并发创建两个 runtime；它不形成新状态机或持久事实。worker join 复用现有 cancellation token 与 task handle，空 claim 复用既有 due 索引，未新增 manager、缓存、队列、数据库字段或依赖。显式 `dev:low-memory` 只为资源受限开发机提供 opt-in，默认开发不再全局牺牲 Cargo 并行度。

2026-09-16 Elicitation 边界修复不新增入口枚举、第二套 command service、响应状态或兼容分支；它复用已有 canonical schema validator 与 IM inbound delivery-action validator，只移除 transport 投影对桌面准入的反向依赖。

2026-09-17 干预上下文补齐不新增 aggregate、持久字段、数据库表、缓存或平台映射。它复用现有 `InterventionPresentation.fields`、linked-detail 双帧和连接器渲染边界，只增加保留字段常量、统一节点解析和显式过滤。

2026-08-31 权限/企微回调补齐不新增展示事实源：`InterventionPrompt.fields` 是 pending params 的一次性 transport projection，按钮容量降级发生在 IM projection/connector 边界，callback 失败态仍复用 outbox 与原 delivery identity。现有 canonical pending state、delivery ID、msgid 幂等和 `InterventionCommandService` 已能表达不变量，因此不为平台容量或解析失败增加第二套状态机。2026-09-01 的短 action 引用与 vote option id 同样只复用 delivery ID 与 outbox action index，不新增映射表、缓存或第二套 token；`ImWeComVoteSelection` 是回调到终态更新之间的一次性 transport context，成功更新后即释放。Permission 双发只新增 connector 内存中的“详情 ACK 后发送卡片”pending 分支，不新增第二条 outbox、平台消息 identity 或持久状态。

2026-09-02 permission occurrence identity 只是把原本隐含在 timeline item ID 中的身份规则显式化：provider 原始 requestId 仍是唯一 Runtime request identity，新增 hash 只服务于 timeline upsert 与 lifecycle/outbox 幂等，不改变审批 aggregate、expected state 或动作提交模型。

2026-09-02 ManualCheck 远程卡片缺口属于正确 Runtime 设计下的 presentation 上下文不完整：`manual_check_pending`、`submit_manual_check` 与 allowed actions 已能表达唯一人工判定，但 snapshot 未携带用户决策所需的最新模型输出，企微也仍使用两个按钮。修复只在 ManualCheck 投影中加入有界模型输出快照，并复用 Permission 已验证的“markdown 详情 -> ACK -> vote 卡 -> ACK”与 4 位 `display_ref` 配对契约；不新增审批状态、第二条 outbox 或展示文案反查。

2026-09-02 ManualCheck“卡片可提交但 Run 不续跑”的根因是共享 `InterventionCommandService::execute` 对 ManualCheck 返回状态错误，而桌面 command 另行走 prepare/resume/commit 两阶段路径，形成入口漂移。修复把桌面与 IM 都收敛到同一个 ManualCheck 应用执行边界，并保留 scheduled occurrence、conversation callbacks、后台 launch ack 与 first-writer lease；这是正确设计下的共享入口实现补齐，不是新增第二种审批流程。

2026-09-02 Permission“IM 已处理但桌面权限卡残留”的根因不是权限响应写入不同：两条入口都使用共享 `execute` 写 response。差异在桌面 command 在 execute 后重建并 emit ACP session 投影，IM 只写 response 未通知前端刷新 pending permission。同轮终态卡片重复可点击与标题丢失 id，分别来自企微协议不支持禁用 `submit_button`、回调上下文未携带 durable `display_ref`。修复统一 Permission 应用执行/收尾边界，终态只禁用协议支持的 checkbox/select，并在标题前置保留 4 位配对编号；这些均为正确领域设计下的 presentation/application 边界实现补齐。

2026-09-03 Elicitation 表单契约只把既有 pending schema 中可安全远程表达的 typed 事实固化为 `RemoteElicitationForm`，并复用当前 delivery 的 allowed action、expected state、outbox 原始值和原始 schema 校验。selector key 与 option id 是平台回调短引用，不新增映射表、缓存、token 或第二身份；unsupported 场景在入队前跳过，不产生平台消息或死信。

2026-09-03 canonical 入站幂等只复用已有 `im_inbound_actions` 审计账本和 canonical event identity，不复制 Runtime 决策、不修改 outbox 状态，也不为每张卡片新增第二张动作表。前序输出是 pending request 投影时的有界 presentation snapshot，不参与 expected state 或 CAS 指纹。

2026-09-03 桌面终态真实复测失败属于企微 connector 的发送分流与局部错误隔离实现不完整：`notification_kind` 只能描述业务类别，不能单独决定 transport 形状；同一个 Elicitation kind 下，待处理 `Intervention` 需要“详情 + 交互卡”，已处理 `InterventionResolution` 只允许非交互 markdown。旧实现仅按 kind 选择 linked-detail，终态 payload 因形状不匹配在发送前返回 `IM_PROTOCOL_INVALID`，该单 delivery 构建错误又退出整个 WebSocket session，导致 terminal outbox 持续得到 `IM_NETWORK_UNAVAILABLE`。发送策略现同时检查 payload 生命周期与 kind；单 delivery 的构建/校验错误只完成该请求的失败结果并继续 session，只有 socket、订阅或平台连接冲突可以改变 channel 连接状态。canonical 审批、terminal outbox 与桌面来源无法伪造 callback 更新原卡的协议结论保持不变。

### 13.3 性能影响评审

企业微信与飞书各最多一个 WebSocket；队列、重试、记录保留均有界。单次 lifecycle 投影为 `O(C)` 且 `C <= 2`；permission 摘要只读取当前 pending request 一次，复杂度为 `O(P + F + A)`（params 条目、投影字段与动作数），路径最多展开 3 条、标题/正文/字段各 256 字符；企微 Permission 双发是当前 delivery 的两个出站帧，详情渲染 `O(F)`、vote 排序仍是 `A <= 20`；elicitation schema 投影为 `O(Q + O)`（问题数加选项数），二者均不扫描 timeline 或历史 Run。回调按 delivery ID 做 indexed 读取，并用该 delivery 的发布动作校验选择和答案；飞书表单最多 5 题，结果内容最多 8 KiB。Runtime/SQLite 工作在 blocking pool 内完成后才发网络响应，网络不位于 SQLite transaction 或领域锁内；热路径不增加全量加载、N+1、无界缓存、长锁或事件线程阻塞。completion 仅携带 canonical event ID、稳定错误码与三个计数，worker 在发送型记录落库后唤醒，tombstone 不唤醒。入站每 action 独立任务，同事件由共享 ActionLocks 去重，飞书回包等待受 2.5 秒预算约束；对应接口回归已覆盖慢处理隔离、容量、批量清理与 generation。

Permission occurrence identity 的生成只对当前事件的两个短字符串做一次长度前缀 BLAKE3 哈希，输出固定 64 hex，避免 provider ID 长度进入 lifecycle key；pending 查找仍是按原始 requestId 的一次文件读取，未增加扫描、缓存或额外 I/O。

2026-09-02 ManualCheck 补充的模型输出读取使用 timeline index：索引扫描只过滤当前 root、可见、非 Agent launch 的 `textDelta` locator，并按最新顺序读取事件到第一个非空输出；输出快照 4096 字符有界，不加载完整 timeline，也不引入缓存。企微双发仍是当前 delivery 的两个出站帧，ManualCheck vote 排序固定为 2 项。

2026-09-03 Elicitation 分类与渲染复杂度为 `O(Q + O)`，Q 不超过 3，vote 选项不超过 20，multiple selector 不超过 3 且每题不超过 10；不枚举多选组合，不扫描 timeline 或历史 Run。回调只解析短 selector/option id，做一次 indexed delivery 读取和一次 schema 校验；每个支持 delivery 仍是两个出站帧，unsupported 场景入队前跳过且不产生重试、死信或平台消息。

2026-09-16 桌面 Elicitation 修复不增加 schema 编译次数、文件读取、网络请求或锁范围：每次 Accept 仍只对当前 pending request 执行一次有界内容大小检查和一次 JSON Schema 校验；IM 回调仍先做 delivery 动作校验再执行同一 canonical schema 校验。没有全量扫描、N+1、缓存、队列或额外持久化。

2026-09-17 干预上下文字段使用当前事件已有的工作空间、任务和节点状态，不扫描 timeline 或历史 Run；AUTO 正常路径为单节点文件读取，完整动态图谱只在节点投影缺失时作为 fallback，卡片字段过滤为常数级遍历，三字段不占横向容量，无新增网络请求、锁、缓存或持久化。

2026-09-03 重复新 msgid 点击增加一次 `channel + canonical_event_id + completed_at` 索引查询；命中后只写一条 AlreadyApplied 审计结果，不进入 Runtime、scheduled resume 或 session 重建。Elicitation 前序输出读取复用 timeline index 的最新 root 文本定位并最多读取一个事件、快照 4096 字符；详情去重只比较当前请求内的 message/description/context。

2026-09-03 桌面终态发送分流只增加一次 enum variant 判断，为 O(1) 且不增加网络帧、SQLite 查询、锁、队列或缓存。局部构建失败在写 socket 前完成对应 oneshot，不占用 pending map，也不触发重连或后续 outbox 的网络失败风暴；正常终态仍为单条 markdown。

2026-09-04 maintenance 的租约扫描和 retention 删除均命中既有索引，单批 200、单轮 4 批；周期分别为 30 秒与 6 小时，不执行 VACUUM 或全量 payload 加载。投影队列 Full 分支只做常数级状态判断、结构化日志和 30 秒冷却的诊断事件，不再执行 Runtime 查询、文件读取或 SQLite 写入。projection barrier 只用于低频删除清理，且不持有 settings/SQLite 锁；重配置按 channel 顺序执行但单个失败不阻断后续 channel，当前仅一个 channel，不引入无收益并行化。

2026-09-07 `claim_due` 的 typed decode 仍受 32 条批次和 32 KiB payload 上限约束，并移到 SQLite immediate transaction 之前；事务只做有索引候选的 rowid/CAS 更新。每次入站动作增加一次小型 settings 读取，settings 锁在 Runtime 执行前释放。pending map 与 `DelayQueue` 同为最多 64 条，超时删除为 `O(log 64)`；空 queue 分支不参与 `select`，不会空转。持续重连最多一个 WebSocket，间隔 60 秒封顶且 generation cancellation 可立即停止，不引入全量扫描、无界内存、长锁或紧密重试。

## 14. 外部协议依据

- 飞书官方 [Node SDK](https://github.com/larksuite/node-sdk)：长连接协议基准；传输使用 tokio-tungstenite，protobuf 使用 prost，openlark 准入未通过。
- 飞书 [消息卡片 PATCH](https://open.feishu.cn/document/server-docs/im-v1/message-card/patch.md)：更新期限、错误码与降级分类。


- 飞书终态控件与企微选择器上限的核对依据见[飞书技术方案 §1](../../开发计划/IM接入/飞书远程干预技术方案.md#1-文档状态)；以组件协议而不是 SDK 展示标签或 PoC 简化形状决定回显字段。
- 企业微信智能机器人 WebSocket：[官方文档](https://developer.work.weixin.qq.com/document/path/101463)
- 企业微信智能机器人官方 Node SDK：[`WecomTeam/aibot-node-sdk`](https://github.com/WecomTeam/aibot-node-sdk)
- 企业微信官方 CLI 扫码授权实现：[`WecomTeam/wecom-cli` `auth/qrcode.rs`](https://github.com/WecomTeam/wecom-cli/blob/main/crates/wecom-cli/src/auth/qrcode.rs)
- 微信官方接入包：[npm `@tencent-weixin/openclaw-weixin`](https://www.npmjs.com/package/@tencent-weixin/openclaw-weixin)

实施时必须再次核对官方权限、限流、回调时限和版本变更，并把依赖精确锁定到审计通过的版本。


## 15. 2026-10-16 跨渠道修复契约

- 飞书追问拒绝使用独立 `decline` wire 意图，进入已有 `ElicitationDecline` 领域动作；拒绝、表单与动作索引不得同时出现，仍由发布动作校验拒绝权限。
- 飞书追问卡片展示投影快照中的“前序模型输出”；空内容省略，提交回显与终态 PATCH 保留该上下文。复用原有最多 4096 字符快照，不另查历史。
- 企微跨端终态提示使用 `resolved_by` 的实际处理来源，支持桌面端、飞书与企微，不固定写为桌面端。
- 飞书 HTTP 响应体中断/超时归为可重试网络失败；收到完整但无效 JSON 才归为协议失败。创建消息携带从 durable delivery ID 派生的 UUID，重试与 token 刷新保持同一 UUID；平台去重窗口为一小时，不能宣称跨窗口恰好投递一次。
- 企微平台卡片标识与本地 delivery 去重身份分开，按 §6 的协议规则创建卡片；回调还原原 delivery，原卡更新保持完整 task/submit key。ACK 不确定时可能出现重复卡片，但 canonical 干预仍只执行一次。42014 保持真实失败，不伪装成功；不重投已完成干预。
- 两个渠道扫码安装新凭据后都清空旧 binding，保留通知偏好。重新授权同一应用也必须重新私聊绑定；设置页只在当前授权 generation 已连接且持久绑定一致时自动关闭，关闭后忽略迟到扫码结果。

影响与风险：改动涉及 connector 协议适配、共享终态文案与授权流程，没有新依赖、表、业务状态机、缓存或队列。企微 wire key 有意升级，升级前仍未处理的旧格式卡片需在桌面端处理，不增加旧协议兼容分支。真实平台证据、本地回归和未验收边界统一见[飞书技术方案 §19](../../开发计划/IM接入/飞书远程干预技术方案.md#19-2026-10-16-六项修复与验收)。

性能复核：上下文渲染最多增加 4096 字符；每次出站只对短 ID 做 UUID 派生，复杂度与当前 delivery 大小相关。企微详情与卡片仍为两个帧，回调仍是一次 indexed delivery 查询；不增加网络轮询、历史扫描、持锁范围或设置页订阅范围。无需为本轮增加缓存或 benchmark。

## 16. 2026-10-16 飞书协议与卡片投影边界

- pbbp2 协议隔离以 `feishu::ws` 模块为边界，允许内部拆文件。connector 消费项目事件和一次性回包接口，低层帧、心跳、分片、socket 与 ACK 由模块内部管理。
- 回包等待任务随所属连接取消；已经提交 Runtime 的领域动作继续遵循既有幂等与持久化语义，终态由原 durable delivery/outcome 补齐，不建立另一套审批状态。
- 出站卡片、回调意图和 toast 用当前实际使用的 typed 组件构建。终态通过已发布选项投影 Runtime 答案，分别设置单选 `initial_index`、多选 `selected_values`、输入框 `default_value` 并禁用；卡片树与回填值仅在本次渲染存在，不持久化。
- 重构前后中英文 36 份完整 JSON 严格相等，保留原卡片字段、控件行为与能力上限；核心 IM 189 项和桌面 IM Runtime 21 项回归通过。两项偏差的实施、静态检查与证据边界见[实施记录](../../开发计划/IM接入/飞书封装与类型化实施记录-2026-10-16.md)。

方案复核：沿用现有 serde、Tokio、prost 和连接生命周期，只类型化实际组件，不增加依赖、持久状态、缓存、队列或网络请求；处理规模仍受当前卡片能力约束，没有新增历史读取或 UI 刷新范围。

## 17. 2026-10-21 飞书扫码应用选择与渠道名称

- 飞书新建应用的机器人名称等于渠道产品名：`wb` 为 `MALING`，`default` 为 `Gold Band`。全局扫码配置只保留描述后缀，不重复保存渠道身份。
- 首次接入和已配置后的“更换飞书应用”使用 `selectOrCreate` 模式，不发送 `clientID` 和 `createOnly`，由飞书原生 launcher 展示已有应用列表与新建入口。
- “重新授权”使用 `reauthorizeCurrent` 模式，只把当前 `public_identity` 作为 `clientID`；该入口用于补权限或恢复凭据，不承担切换应用职责。
- 更换应用完成后沿用既有扫码提交事务：替换凭据、清除旧私聊 binding、保留通知偏好，并等待新账号完成私聊绑定。取消扫码不修改当前配置。
- 不新增应用清单接口、缓存、数据库字段或持久状态；应用选择完全由飞书平台承担，前端只保留单次扫码模式。

方案复核：新增复杂度仅为命令层类型化模式和一条配置描述后缀，没有新增网络请求、全量扫描、状态订阅或无界并发。
