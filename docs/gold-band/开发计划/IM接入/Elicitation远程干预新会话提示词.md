# Elicitation 远程干预新会话提示词

> 用途：新开 Codex 会话时，将本文档的提示词完整发送给新会话，用于先评估再实施 IM 远程处理 ACP Elicitation 的技术方案。
>
> 状态：已评估并实施；真实企微 `vote mode=1` / `multiple_interaction` PoC 与多端验收待完成
>
> 日期：2026-09-02；实施记录更新于 2026-09-03

## 提示词

```text
你在 D:\IdeaProjects\Gold-Band 项目中工作。目标是对 IM 远程处理 ACP Elicitation 的技术方案先做完整评估，再实施。开始前必须先读取并遵守 AGENTS.md；本任务涉及状态、生命周期、数据完整性和异步竞态，还必须完整读取 docs/gold-band/rules/state-lifecycle-and-data-integrity.md。同时读取以下文档和代码，不要假设下述现状仍然成立：

必读文档：
- docs/gold-band/产品设计文档/integration/im-remote-intervention.md
- docs/gold-band/开发计划/IM接入/客户端直连IM远程干预实施计划.md
- docs/gold-band/开发计划/新增流程/AskUserQuestion-适配方案.md
- docs/gold-band/开发计划/新增流程/AskUserQuestion-适配优化方案.md

必读代码：
- src/app/intervention.rs
- src/acp/elicitation.rs
- src/im/projection.rs
- src/im/model.rs
- src/im/inbound.rs
- src/im/connector.rs
- src/im/connectors/wecom.rs
- src-tauri/src/im_runtime.rs
- src-tauri/src/commands.rs 中 respond_elicitation、execute_intervention_command、Permission/ManualCheck 的共享执行边界

任务背景：
Permission 和 ManualCheck 的企微远程审批已经基本完善，使用了“markdown 详情 -> 等待详情 ACK -> 交互卡 -> 等待卡片 ACK”的模式，并且桌面端与 IM 端已经收敛到共享应用执行边界。现在要把 Elicitation 按同样架构补齐。当前 Elicitation 实现仍主要面向单题单选按钮，无法完整表达多选和多问题表单，且桌面与 IM 收尾边界还未完全统一。

必须实现的远程范围，仅企业微信处理：
1. 单题单选，固定 scalar 选项，无 custom answer：
   - markdown 详情 + vote_interaction mode=0。
2. 单题单选，固定 scalar 选项，带 custom answer：
   - 处理方式与无 custom answer 的固定选项一致；
   - IM 只提交固定选项；
   - 自定义答案仍去桌面端填写。
3. 单题多选，固定 scalar 选项：
   - markdown 详情 + vote_interaction mode=1；
   - 提交 content 为 { fieldName: [values...] }。
4. 2-3 个单选题，固定 scalar 选项：
   - markdown 详情 + multiple_interaction；
   - custom answer 是否存在不影响分类，处理方式与不带 custom answer 完全一致；
   - custom answer companion field 一律从 IM 表单中移除；
   - 只提交各主问题的固定 scalar 值；
   - 移除 custom companion 后构造出的对象必须仍能通过原始 requestedSchema 校验。
5. 其他情况一律不发送消息给 IM：
   - 不生成 Elicitation IM outbox 行；
   - 不发送 markdown；
   - 不发送提示卡；
   - 不产生 IM 死信或重试；
   - 桌面端原生 Elicitation 处理不受影响。

按最新要求，IM 卡片不提供通用 Decline 按钮；Decline 仍在桌面端处理。不要为了拒绝按钮占用表单选项容量或混用控件语义。

企微控件约束：
- vote_interaction option.text 建议最多 11 个字。
- multiple_interaction select option.text 建议最多 10 个字。
- selector title 建议最多 13 个字。
- vote_interaction 最多 20 个选项。
- multiple_interaction 最多 3 个 selector，每个 selector 最多 10 个选项。
- card title 建议最多 26 个字，desc 最多 30 个字。
- sub_title_text 建议最多 112 个字。
- option.id / submit key 有协议长度和字符约束，只能使用本地短 index 和 delivery identity。
- task_id 继续使用 deterministic delivery identity。
- submit_button.key 继续使用 <delivery_id>:submit。

大小策略：
- 支持的 Elicitation 场景中，markdown 详情或卡片 payload 超过企微 32 KiB 时，必须截断后继续发送 IM，不允许因超限跳过。
- markdown 在 UTF-8 安全边界截断，可保留“内容已截断”提示。
- 卡片 payload 禁止对序列化后的 JSON 字符串做字节级截断；必须在构造前和构造后对展示型字符串做有界处理并重建卡片。
- 不得截断 task_id、delivery_id、submit key、question_key、option.id 等协议或语义 identity。
- 卡片 label 可以截断，但 outbox 仍必须保存完整 typed scalar value；用户点击短 option.id 后从 outbox 还原原始值并构造 schema content。

数据与领域设计要求：
- PendingElicitationState 和 requestedSchema 仍是唯一权威事实源。
- IM 不保存第二套审批状态，不推导 Runtime 终态。
- 复用现有 outbox、delivery identity、msgid 幂等、expected state、InterventionCommandService。
- 不做自然语言解析，不做 IM 草稿状态机，不做多卡片向导，不新增第二套 outbox。
- 多选和多问题不得枚举 2^N 组合动作。需要引入 channel-agnostic 的固定表单契约，例如 RemoteElicitationForm：
  - SingleScalarChoice：一个字段和固定 scalar 选项；
  - MultiScalarChoice：一个字段和固定 scalar 选项；
  - ScalarChoiceQuestions：2-3 个单选字段，每个字段有 selector key、field name、固定选项和 required 状态。
- 可将该契约挂在 InterventionAllowedAction::ElicitationFixedForm 或等价 typed 结构中，并纳入 expected state；timelineIdentity 仍不得参与 expected state 指纹。
- 回调选择需要新的泛型 Form selection，例如 ImInboundActionSelection::Form { selections }，每个 selection 含 selector_key 和 option_ids。
- 企微 connector 只把平台回调转换为泛型 Form selection，平台 DTO 不进入领域层。
- callback 必须校验 msgid、私聊 actor/conversation、delivery/task、submit key、question key、option id、required selector、重复选项和非法 index。
- 多选允许空数组仅当原始 schema 允许且平台回调显式返回空选择；否则拒绝。
- 最终构造 InterventionAction::Elicitation { action: Accept, content }，继续用原始 requestedSchema 编译校验。

custom answer 规则：
- 通过 _meta._askUserQuestionCustomAnswer.isCustomAnswer=true 识别 companion field。
- companion field 不作为独立问题展示。
- 单题单选带 custom answer 时，IM 仍展示并提交固定选项；自定义答案去桌面端。
- 2-3 个单选题时，custom answer 是否存在不影响分类；处理方式与不带完全一致，IM 移除 companion field，只提交主字段固定选项。
- 若移除 companion 后固定选项对象无法通过原始 schema 校验，该请求不发送 IM。

出站卡要求：
- Elicitation 纳入 Permission/ManualCheck 的 linked-detail 模式。
- markdown 详情和交互卡标题使用同一 durable display_ref，格式参考“补充信息（id=xxxx）”。
- markdown 详情展示任务、节点、message、每题标题、描述、题型、完整固定选项和说明；custom answer 场景可说明自定义答案需桌面端填写。
- vote mode=0 使用 checkbox.mode=0。
- 单题多选使用 checkbox.mode=1。
- 2-3 个单选题使用 card_type=multiple_interaction，select_list 分别映射 q0/q1/q2。
- option.id 只使用本地短 index，不携带业务 JSON、HMAC token 或长值。
- 详情 ACK 成功后才发送交互卡；卡片 ACK 成功后才把 delivery 标记 sent。

终态更新要求：
- 保持原 card_type、原 task_id、原 submit_button.key。
- vote mode=0：`checkbox.disable=true`，最终选项 `is_checked=true`。
- vote mode=1：`checkbox.disable=true`，所有被提交选项 `is_checked=true`。
- multiple_interaction：每个 `select_list[].disable=true`，`selected_id` 指向最终选择。
- 官方 `submit_button` 只有 `text/key`，没有 `disable` 字段；终态保留原 key，但不得写入无效的 `submit_button.disable`。
- 终态标题必须把 `id=xxxx` 放在最前，例如“id=0112 已处理：<答案摘要>”；答案摘要可截断，编号不得截断。
- 重复同一 `msgid` 复用入站幂等结果；同一 canonical intervention 即使产生新 `msgid`，也按 `channel + canonical_event_id` 收敛为 `ALREADY_APPLIED`，不重复执行 Runtime。
- AlreadyHandled / RevisionConflict 的 IM 卡终态收敛为“已处理”；业务失败则更新失败态，但仍保持原卡结构。
- 禁止把可交互 vote/multiple 卡降级为缺少原控件的 text_notice。

桌面端与 IM 统一执行边界，这是本任务的关键：
- 当前桌面 respond_elicitation 有 reclaim scheduled interaction、record ElicitationResolved、共享 execute、重建 session VM、emit session update 等收尾；IM 的 execute_intervention_command 对 Elicitation 可能仍走普通 execute 分支。
- 必须抽出或补齐 execute_elicitation_intervention 应用边界，并让桌面 command 与 IM inbound 都调用同一条边界。
- 边界顺序必须保持：inspect/校验 -> 在 response signal 可见前 reclaim scheduled interaction -> record ResumeCause::ElicitationResolved -> 共享 InterventionCommandService::execute 写 response signal -> 重建 root/dynamic ACP session VM -> emit session update 清理桌面 pendingElicitations -> 触发 attempt index 更新。
- 不要为 Elicitation 手动伪造 workflow 续跑；自然续跑仍由 ACP waiter 消费 response、发送 JSON-RPC response 并清理 signal。
- 桌面先处理、IM 后点击时返回 AlreadyApplied，但仍要 emit 最新 session 投影并更新 IM 卡为已处理。
- 并发桌面/IM 提交必须 first-writer-wins。

必须规避的历史问题：
1. ManualCheck 审批后不续跑：
   - 根因是 IM 与桌面入口执行边界漂移；
   - Elicitation 不得复制只写状态不收尾的路径。
2. 终态 vote 卡可重复提交：
   - 协议支持的 checkbox/select 必须显式 disable；submit 没有 disable 字段，必须依靠 `channel + canonical_event_id` 幂等收敛新 msgid，禁止再次执行 Runtime。
3. 终态标题丢失 id=xxxx：
   - response context 必须从当前 outbox enrich durable display_ref。
4. Permission 桌面端和 IM 端没有同一条完整收尾边界，导致桌面权限卡残留：
   - Elicitation 必须统一应用边界，并在提交后 emit ACP session update，确保桌面 pendingElicitations 立即消失。

协议 PoC 闸门：
- vote mode=0 已有 Permission 真实 PoC 基础。
- vote mode=1 和 multiple_interaction 的真实回调形状尚未完全确认，不能用 SDK 类型猜测后直接宣称生产可用。
- 实施时可以补生产实现与接口 fixture，但必须另做真实企微手机端和 PC 端 PoC，采样脱敏后的 selected_items 结构、question key、option id、task id 和 event key。
- 未通过真实 PoC 前，不得宣称真实平台验收完成。

实施顺序建议：
1. 复核现状与方案，输出根因判断、方案、过度设计评审、性能影响评审和测试计划；先给用户确认。
2. 更新产品设计文档和开发计划，明确五类范围、32 KiB 截断策略、控件契约和终态要求。
3. 实现领域侧 typed form projection 与 expected state。
4. 实现 IM projection 分类；支持场景才入 outbox，其他场景无 delivery。
5. 实现企微 markdown + vote/multiple 出站卡。
6. 实现企微回调解析、泛型 Form selection、outbox 还原原始值和 schema 校验。
7. 抽出并统一 Elicitation 桌面/IM 应用执行边界。
8. 实现终态卡更新，保留原控件、task/submit key、display_ref，并全部禁用。
9. 补齐单元测试、接口测试和文档记录。
10. 用真实企微完成 mode=1、multiple_interaction、多端点击、并发桌面/IM、5 秒终态更新和重复回调验收。

验收测试至少覆盖：
- 五类范围分类：支持场景入 outbox，其他场景无 outbox 行。
- 单题单选无 custom、带 custom、单题多选、2/3 个单选题。
- custom companion 移除后 schema 仍合法；不合法则不发送 IM。
- markdown 和卡片超 32 KiB 时截断发送，不跳过。
- 截断 JSON 不产生非法协议帧，identity 字段不变。
- 点击截断 label 后仍提交完整原始 scalar value。
- vote mode=0/1 和 multiple_interaction 的出站 payload 形状。
- 回调缺失 selector、重复 option、非法 option、task 不一致、msgid 重复、expected state 冲突。
- 多选空数组是否按 schema 允许执行。
- 桌面先处理、IM 先处理、并发双入口 first-writer-wins。
- IM 处理后桌面 pendingElicitations 立即消失。
- scheduled interaction reclaim 和 ElicitationResolved metrics cause。
- 终态卡保留原类型、原 task/submit key、display_ref，并禁用后不可重复提交。
- 真实企微手机端和 PC 端验收。

性能与过度设计要求：
- 分类与渲染复杂度保持 O(Q + O)，Q 为问题数，O 为选项数。
- 不枚举多选组合，不引入无界缓存、第二状态机、第二 outbox、平台消息映射表或自然语言解析。
- 每个支持 delivery 仍最多两个出站帧。
- 回调只解析短 question/option id，做一次 indexed delivery 读取和一次 schema 校验，不扫描 timeline 或历史 Run。
- unsupported 场景入队前跳过，不产生重试、死信或平台消息。
- 必须在方案和完成验收时输出过度设计评审与性能影响评审。

建议验证命令：
- cargo fmt --all --check
- cargo check --lib -j 1
- cargo check -p gold-band-desktop -j 1
- CARGO_PROFILE_TEST_DEBUG=0 cargo test app::intervention --lib -j 1
- CARGO_PROFILE_TEST_DEBUG=0 cargo test im:: --lib -j 1
- CARGO_PROFILE_TEST_DEBUG=0 cargo test -p gold-band-desktop im_runtime::tests -j 1
- git diff --check

输出要求：
- 先给出技术方案评估和实施计划，说明根因属于原有设计缺陷、正确设计但实现不完整，还是实现逻辑错误，并等待用户确认后再改代码。
- 每次代码修改同步维护产品设计文档和开发计划。
- 不引入与目标无关的重构。
- 不回退用户已有修改。
- 完成后明确列出测试结果、未完成的真实企微验收和剩余风险。
```
