# Direct 会话后台消息与常驻保活

状态：实现完成，验收结果见下。日期：2026-10-09。

## 后台副作用补齐（2026-10-09）

- 普通 prompt 的结束、队列与通知保持独立。后台消息不新建用户 turn，也不把静默当作任务完成。
- Direct 统计主体保持 Task UUID；正常完成立即上报，后台新增可上报统计补发更高 revision 的累计 ExecutionCompleted，正在执行用户 prompt 时合并到正常完成。重试同一事实保持幂等，不增加 followUpCount。上下文 used/size 不转换成 token 消耗；适配器未提供的用量保持未知。
- 后台正文按已读到未读的变化提醒一次；未读期间后续回复合并，读取后重新允许提醒。同一正文的流式续写不反复提醒。普通 prompt 期间交错输出不发第二类通知；权限/问题仍独立提醒。现有窗口前台抑制规则继续有效。
- 活动摘要使用已接收的 live item 身份展示正在思考/调用工具；历史载入不重播活动，旧 prompt 终态不被改回 running。
- 后台权限/问题统计不依赖 active user turn；工具文件变更继续使用既有有界采集 worker。
- 过度设计评审：复用时间线、attention、metrics collector 与 native notification，不增加后台调度器或虚构的任务完成状态。必要的统计快照是可恢复的报告投影，不是第二份运行状态。
- 性能评审：live 合并继续使用既有有界缓冲；通知按逻辑正文去重，统计补报按变化合并；不逐 chunk 读全量历史或发送网络请求。

### 实现契约与实际协议边界

1. 通知按持久化未读周期合并，不再采用 2 秒静默/10 秒上限，因此无需增加通知计时配置。首次非空后台 Agent 正文进入未读时提醒一次；同一逻辑消息后续 chunk 即使用户已经读过也不重新提醒。读到更新回复后才重新允许下一条回复提醒。未读错误/停止提示保持原语义，ACK token 随新回复推进，迟到 ACK 不清除更新消息。
2. 后台消息的 ACK 需要匹配已载入会话、窗口可见且聚焦、消息窗口处于最新底部。仅停留在会话路由、最小化或上翻历史不算已读。原生通知继续复用当前会话在前台时的抑制逻辑。普通 prompt 的 end_turn/队列完成通知独立，后台不会合成 Finished。
3. Direct 的 executionId/attemptId 均沿用 Task UUID，attemptIndex 为 1。正常结束用 prompt usage journal 的全部已确认分段重建累计值，修复第二轮只上报本轮消耗的问题。每次正常终态保存一份最近 terminal fact，后台权限/问题计数变化时复用该结果补发 ExecutionCompleted；新 factId 交由既有 collector 分配更高 revision，重复事实幂等。用户下一轮活跃时仅记录 intervention，最终并入正常结束统计，不增加 followUpCount。
4. 现有接收协议已经要求按更高 revision 替换终态累计快照，无需新增接收端 API。此次没有验证线上部署版本。通用 ACP `usage_update.used/size` 是上下文占用，示例日志的 cost 也没有对应现有采集字段；不会将这些当成消耗 token，也不会为纯正文、工具进度或上下文心跳发送无变化的统计。后台真实 token 缺少标准数据时保持未知，不能承诺补齐 Agent 未提供的消耗。
5. 后台工具 diff 沿用最近持久用户 prompt identity。仅 Direct 的 worker 保留有界 capture checkpoint（mutation 引用、已知工具结果及预算），解决 end_turn 后重建 worker 丢失前段变更的问题。工具明确终态后发布累计文件结果；下一 prompt 接管前 drain worker。失败工具不提交修改，反向编辑将先前结果抵消时发布空替换。checkpoint 不含文件正文，也不需要回放完整消息历史；单槽输出只保留最新累计结果。
6. live 活动投影只由本次实际收到的 activity 身份激活，保留多工具并行时仍未结束的工具。历史加载保持静态。session-owned permission/elicitation 不被上一 prompt 的终态清除。

### 本轮验证

- 修复前失败证据：累计第二轮上报 20（预期 30）；隐藏窗口误清后台未读；上一 prompt 终态清除 session-owned 交互；最新工具结束使另一未完成工具被折叠；后台继续编辑丢失初始文件版本；后台反向编辑未清除旧结果。以上均以对应回归测试验证修复。
- 前端相关测试 122 项通过，TypeScript 检查通过；生产 Vite 构建通过。浏览器直接渲染实际 ACPMessageList，completed 会话收到工具 in_progress 显示“正在调用”，工具 completed 后显示“已记录”；宽窄窗口无水平溢出。
- Direct 核心测试 25 项通过、2 项环境测试忽略；文件 worker 10 项通过；attention 5 项、metrics collector 17 项、通知前台判断 3 项、原有通知 17 项通过（原生注册表测试 1 项忽略）。前端验证进程与浏览器已关闭。
- 环境：E 盘空间不足导致构建和一次源文件写入失败；源文件已按原差异快照及操作记录恢复核对，构建/恢复副本转至 F 盘。没有清理用户数据，没有重新调用真实付费 Agent；本轮使用已有 ACP mock 协议集成测试。

### 设计与性能复核

- 属于既有会话消费设计的副作用实现缺口，复用现有 timeline、attention、metrics fact/collector、file worker，没有新依赖、后台任务状态机或按静默完成任务的机制。
- 新增报告快照用于在没有 active turn 时恢复最近真实完成结果；有界文件 checkpoint 保存 journal 缺少的工具结果与采集预算，避免重建时扫描历史猜测成功。两者都不是新的用户回合或任务身份。
- 通知每条逻辑消息最多一次持久判定；统计只在正常结束或实际 intervention 变化时运行，沿用已有串行有界 metrics worker。正常结束读取 usage journal 的量与既有实现相同；补报读取单个最近报告快照，不扫 timeline。文件计算在独立有界 worker，UI 只接收最新累计文件结果；状态锁不覆盖原生通知发送。

## 2026-10-14：官方子会话状态接入

- 现场：task-314 的子 Agent 已由 provider 明确通知完成，旧 ACP transcript 只落下报告正文，没有正式分支完成事实，父 prompt 结束后按旧推断显示 Interrupted。
- 核查修正：正常同步路径没有被后台改动整体破坏。真实持久化入口回归“子正文先落盘并退出热状态，再收到同文同步正式结果两次”在原实现上通过；因此不修改该路径，不把热状态过滤或旧 schema 迁移的静态疑点当作此次根因。
- 上游方案：使用官方 Claude ACP 0.90.0 的 ACP v1 subagents RFD，不伪装 AIR、不 fork adapter、不建立任务查询或轮询系统。此次仅隔离安装验证，不修改用户现有 adapter、EXE、配置或历史会话；使用新协议需要客户端代码与支持该能力的 adapter 同时具备。0.87.0 也接受同名 capability，但使用不兼容旧草案，因此正式 initialize 按真实 agentInfo 校验官方 Claude ACP ≥ 0.90.0，低版本返回结构化升级错误；其他 provider 不套用 Claude 版本规则，七语言升级文案测试通过。
- 数据边界：child session 是传输地址，原根 session + launch tool ID 仍生成同一 canonical execution。连接 ingress 在公告时建立父 pump 别名，consumer 用 session_message 证明 launch 关联；别名随 owner/generation 清理，正文/权限仍归属原分支。待关联公告有界，不缓存全历史、不按名称猜测关系。
- 状态边界：内容为空的 agentState 是现有 timeline 的官方状态事实；索引/实时视图读取最新状态及 sequence，明确 running 可以恢复同一 child。普通文本、异步启动回执和父 end_turn 均不代替子终态。省略 state 的更新保持原状态，显式 null/未知结束原因显示已有“状态未知”。真实父运行时关闭保留未完成子执行的中断收敛，已有完成不被覆盖。
- RED/GREEN：旧 connection 在父公告之后收不到 child session_message（Err(Empty)）；旧分支投影把官方 running 覆盖成 interrupted；旧 client 只保存初始 Prompt 而没有 agentState。相同接口测试经路由、归一化和索引修复后转绿。额外分别复现并修复元数据更新清空官方状态、完整重放正文被再次追加；没有用普通文本猜完成。
- 官方包探针：真实 ACP 0.90.0 + SDK 0.3.296 / CLI 2.1.296，通过 Gold Band AdapterConnection 和 localhost 模型 fixture 验证 capability-off 同步、capability-on、异步完成、取消、子权限、SendMessage、close→load replay 共 7 场景，13.16 秒、23 次本地 fixture 响应，无付费模型请求。取消/权限保留真实 child session ID；重放恢复原 child ID 和结束原因。该探针单独证明上游协议，不冒称生产 mapper 已集成。
- 生产消费入口：将官方 on/async/cancel/resume/replay 五组真实抓帧送入 handle_session_update，再落盘冷读 canonical branch，验证最终状态、单次正文、父输出隔离及无 native 拒收诊断；该显式回放测试通过，补足协议探针不经过生产 mapper 的边界。原同步结果持久化回归保持通过。
- 前端：复用原 Agent 卡片、Tab、未知文案和有界 live cache；providerStateSeq 只比较官方状态事实，不作为全部摘要字段的 revision，attention 仍独立消费未决交互。相关 6 文件 197 项通过；最终错误文案/i18n/router/chat/panel 五文件定向测试 177 项通过（与前述有重叠，不累加）；TypeScript 检查和最终生产构建通过。独立浏览器使用实际 ACPChatDialog 验证 completed→running→unknown→completed、父 end_turn 不覆盖 child、单卡片/DOM identity 不变、状态事实不显示为正文；390px 与 1100px 无横向溢出。验证进程、浏览器、临时页面及本次隔离安装的 adapter 已清理。
- 最终 Rust 回归：启用正式 capability 和版本校验后，`cargo test -p gold-band --lib acp::` 为 650 通过、3 失败、7 忽略；新增 native 状态、路由、重放、版本边界及原同步回归均通过。失败一项是未变更模板的 checkout CRLF 与测试预期 LF 不同，同一 binary 定向重现；另两项 doctor 测试未在共享 deadline 内达到预期 fixture 阶段，同一 binary 单独及串行各重跑一次均通过，但未证实全量并发失败的精确原因，不能宣称全套通过。`cargo check -p gold-band-desktop` 通过；桌面 binary 的 `view_models::tests::agent_execution_vm_does_not_serialize_provider_tool_ids` 单测通过，确认 providerStateSeq 序列化且不泄漏 provider tool ID。
- 已知验证边界：扩展前端重入测试有既有失败 `atomically rejoins the live head with replay received during the latest-page request`（期待 Editing · files）；隔离加载 HEAD 原源码也失败，非本次引入。未运行付费线上模型、真实 provider 嵌套子会话、父 idle 后后台子任务取消、跨进程重启子会话或修复 task-314 旧数据；官方重载后继续子会话的能力仍受上游实现限制。
- 过度设计与性能复核：没有新依赖、任务数据库、独立执行器、状态机、历史扫描器或后台调度器。复用原消息队列、分支身份、timeline 和索引；每个状态增量只更新所属分支。子路由/绑定上限 1024、待关联公告 64 条且单条最多 16 KiB；已绑定正文按 ID 直接路由，新增别名容量核查和 owner 清理才扫描当前连接路由，未在逐 chunk 热路径增加历史 I/O、N+1 查询或无界缓存。必要映射是有界协议投影，不是平行 canonical state；未发现需新增专项 benchmark 的性能风险。

## 2026-10-15：后台唤醒的计时、Token 说明与引用配套

- 现场：task-105 的 prompt 实际 13 秒，约 10 分钟后定时唤醒工作 18 秒；时间线会话累计记为 658 秒（= 最后后台事件 − prompt 开始），`acp.snapshot.json` 仍是 13 秒、累计费用 $0.0761（raw 已到 $0.0945）。后台回复最后一条消息一直处于流式状态，不能引用或复制。
- 根因一（设计缺陷，计时）：`AcpTimingState` 只有用户 prompt 一种计时单元，end_turn 不关闭本轮；后台事件照常推进上一轮最后活动时间，空闲间隔被并入，下一次 prompt 还会把它一次性折算进累计。修复：后台投递（`_meta.goldBandBackground`）先在上一轮自身最后活动处关闭该轮，再按独立后台片段计时；相邻后台活动间隔超过 `BACKGROUND_TIMING_IDLE_GAP_SECS`（10 秒）结束片段，in_progress 工具期间静默照算；新 prompt 与后台停止标记结算片段。片段状态进入 `AcpTimingStateSnapshot`（serde 默认值，旧快照不迁移）。合并条目的 `timestamp` 停在首个 chunk，活动时间统一取 `max(timestamp, endedAt)` 且只前进，修复 usage 合并条目把前台最后活动拉回的问题。
- 根因二（实现缺口，快照）：快照只在 prompt 终态写入，后台没有终态。后台 usage_update 到达时用 `patch_session_metadata` 刷新快照的 usedTokens / contextWindowSize / totalCostUsd / timing；锁与生命周期 header 沿用既有事务。
- Token：分项只来自 prompt 响应，后台无法补齐。runtime 观察到后台 Agent 工作（与后台活动窗口同一判断）后，在 usage_update `_meta.goldBandTokenTotalsExcludeBackground` 持久化该事实；index usage 投影只置位，VM `tokenTotalsExcludeBackground`，前端实时 reducer 同样只置位；tooltip 在 Token 分项下方显示“不含后台唤醒的消耗”，七语言同步。
- 根因三（实现缺口，引用）：流式标记只在 `sessionActive` 变为 false 时结算，后台回复期间它一直为 false，结算永不触发。改为前台 turn 与后台活动（复用 `useDirectBackgroundActive`）都结束时结算。
- RED → GREEN：计时 4 项（idle gap 800→31、下一轮折算 663→36、运行工具 720→93、快照往返 662→35）；Direct fixture 集成测试去掉修复后在标记断言失败；前端后台回复 10 秒窗口结束后仍 `data-streaming=true`。修复后全部转绿；task-105 真实时间线按到达顺序回放为 31 秒。
- 回归：`cargo test -p gold-band --lib -- acp:: timeline personal_analytics` 674 通过，3 项失败与既有基线一致（提示词 CRLF 与两项 doctor 截止时间，后两项单独重跑通过）；前端相关 9 个文件 208 项通过，唯一失败为既有 `atomically rejoins the live head…`（去掉本次改动同样失败）；`tsc -p web/tsconfig.build.json` 与 `cargo check -p gold-band-desktop` 通过；桌面端 usage 相关 10 项（含新增 VM 序列化测试）通过。
- 未覆盖：metrics 上报的 `timing` 仍沿用最近一次 prompt 终态快照，后台片段不单独补报（改动对外上报口径，留待确认）；未执行真实 Claude 定时唤醒端到端重跑；浏览器实测因 agent-browser 在本机打开/连接 CDP 均超时未完成，tooltip 与引用由 DOM 测试覆盖，测试进程、Chrome 与 1420 服务已清理。
- 自评审：没有新增状态机、依赖或轮询；计时只增加常量规模字段，后台片段判断为 O(1)，工具集合按并行后台工具数有界。快照刷新只在后台 usage_update 时发生（task-105 后台 18 秒内 2 次），每次一次小 JSON 读写；index 投影只多一个布尔位。

## 目标与边界

将 Direct 的消息消费生命周期提升到 ACP 会话层，支持 prompt 返回 `end_turn` 后继续接收自主输出、后台工具通知和交互请求。页面切换仅释放展示资源。Workflow、AUTO 的执行终局、后台能力限制及回收策略不变。

复用现有连接管理、运行时注册表、完整 locator、connection/route generation、事件 sequence、时间线存储、权限交互与 UI 组件，不新建调度平台或第二套执行状态机。Zed 的会话级消息分发和空闲保留为参考；ACP 没有完整通用的隐藏定时任务清单，不能将未知解释为无任务。

## 配置

唯一默认值来源为 `configs/app-config.toml`：

```toml
acpSessionIdleTtlSecs = 600
acpMaxIdleSessionRuntimes = 8
acpSessionSweepIntervalSecs = 60
acpDirectBackgroundStopGraceSecs = 10
```

2026-10-10 起 Direct 与 Workflow/AUTO 共用 acpSessionIdleTtlSecs / acpMaxIdleSessionRuntimes，原 `[acpDirectSessionRetention]`（residentThreshold=8、6 小时 TTL、capacityTarget=20）删除，不保留兼容读取。sweep 间隔与后台停止宽限为正数时生效，否则使用默认值。配置经现有编译内嵌链路加载；修改后重新构建，不增加热加载。无人使用的适配器连接仍沿用现有回收。

## 生命周期与回收

- Direct 与 Workflow/AUTO 存活会话进入同一候选集合：受保护会话既不过期也不占空闲名额；其余空闲会话超过 600 秒回收，空闲会话合计超过 8 个时按最后有效活动时间从最久的开始回收。
- 手动常驻、正在执行/已接纳输入、等待正式权限或问题响应、可信的未完成工作、关闭中、共享连接别名（同一 connection generation + sessionId 被多个注册项引用）、有效前台租约均保护；其中前台租约与正在执行对两类会话通用，其余为 Direct 独有。普通文本提问后 end_turn 不等于待交互状态。
- 有效用户/Agent 内容、工具进展和交互续期；历史重放、重复的纯工具状态、usage/页面心跳不续期；协议没有去重身份的相同正文 chunk 不能猜测为重复。已知执行结束重新开始完整空闲期。
- 一个应用级 sweep 线程（间隔 60 秒，注册表清空即退出，下次 release / 启动 Direct 消费者时按需重启），release 时也立即检查一次；仅扫描存活注册表，不为每个会话创建回收定时器，不扫描历史。
- 8 是空闲上限；所有会话受保护时允许超出，不强停用户任务，也不承诺内存硬上限。
- 回收前在同一生命周期同步边界重查代次、活动、常驻和执行状态；不长时间持全局锁等待关闭。关闭失败不得伪装资源已释放。
- 按 capability 使用 session/close，保留关闭期间消息处理。共享连接不得因某个 Direct 会话回收而关闭。无独立 close 能力时仅能在整个连接安全可释放时关闭进程，否则保留并记录原因。

## 常驻交互

仅 Direct 右键显示一个状态相关操作：未常驻为“常驻会话”，已常驻为“解除常驻”。标题旁小图钉与共享 Tooltip 表示已常驻；不改变排序，和现有置顶独立。

Direct 会话头部“原始帧”左侧提供同效图钉切换按钮（`ResidentPinToggle`）：未常驻为描边图钉，已常驻为与侧栏一致的渐变实心图钉；`aria-pressed` 表达状态，Tooltip 提示“有定时任务等长时间工作时开启常驻 / 已常驻，点击解除”。状态取自侧边栏会话行，回调复用 App 的 set_conversation_resident 处理（错误由 App 统一展示）；请求中禁用并显示加载态；只读体验或会话行未加载时不显示。

常驻偏好绑定现有 project_id + task_id，持久化于现有 authoring/conversation.json 的 resident 字段，通过 App.set_conversation_resident 写入，并与活动时间更新共用文件锁。该偏好表达现有置顶顺序无法表达的资源保留意图，不新增会话 identity。重命名不丢失；删除清理；复制默认不常驻。给历史会话设置偏好不启动 Agent，应用重启不自动启动常驻会话。解除后参与正常检查，不直接关闭；执行/等待交互仍保护。

## 消息与状态归属

每个会话唯一有序消费者，复用原事件泵与有界队列。消息先写 canonical timeline，再发布对应 UI 增量。页面只订阅，不竞争消费。end_turn 仅完成对应 prompt；后续内容不能回滚 Run/Attempt 终态、伪造用户输入或归入下一 prompt。

按 sessionId 路由，按 messageId/toolCallId 合并；Agent 缺少身份时保持输出顺序，不猜测归属。新 prompt 接纳前的合法自主输出不能一律隔离丢弃；历史重放继续按身份和水位去重。Direct 权限/问题请求均绑定真实会话与请求身份（包括前台 prompt 执行期间，避免猜测并发请求归属），等待期间保护，不自动批准、不因缺少用户 prompt 自动取消。

## 资源边界

释放页面/消息缓存不关闭 Agent 会话。session/close 会取消会话工作；Claude 适配器还会关闭该会话 SDK query/子进程，外层适配器存活不能保证定时器继续。未知、长于 TTL 的定时任务可能因回收中断，用户常驻提供进程运行期间的保护。恢复历史不是通用的后台任务恢复保证。

## 实施与验收

- [x] 配置加载、校验和常驻状态/接口。
- [x] Direct 持续接收、交错消息及后台交互。
- [x] 分级回收、周期维护和并发保护。
- [x] Direct 图钉/右键、七语言文案、置顶隔离。
- [x] 接口测试：8/9/20/21、滑动续期、保护、去重、跨项目、Workflow/AUTO 隔离。
- [x] 模拟 ACP：end_turn 后自主输出、前后台交错、后台权限、发送接管时常驻保留、回收后重新注册、关闭末尾消息排空、关闭不支持/失败。
- [x] 前端类型检查、生产构建、浏览器明暗主题与窄窗口验证。
- [x] 真实 Agent 定时唤醒验证，记录版本；不能以模拟测试替代。
- [x] 1/8/20 模拟会话资源诊断与既有事件队列回归，记录结果及限制。
- [x] 同步产品设计、清理本次测试资源。

## 自评审

过度设计：复用现有身份、序列、注册表和存储；仅新增 Direct 策略配置与独立常驻意图，后台执行与 prompt 生命周期解耦是必要边界修复。通用功能不依赖 Claude 私有任务事件。

性能：维护仅访问存活注册表；常见 N 为 8～20，候选排序 O(N log N)，每次关闭前重查，整批最坏 O(N² log N)，活动状态更新 O(1)。历史与大正文不常驻前端，队列/缓冲有界。风险集中在 Agent 进程树内存和停顿消费者，需生产构建与压力场景验证；保护会话可超目标，不能声称绝对内存安全。

## 验收记录（2026-10-09）

- 最小失败测试先复现已落盘 messageId 的后台续写被当成重放丢弃，再验证修复；扩展测试进一步复现新 prompt 后旧消息被覆盖，修复为按项索引恢复后追加。
- 模拟 ACP 往返覆盖：end_turn 后续写、后台权限确认、会话级问题回答、下一轮 prompt 接管、常驻状态不因接管丢失、不支持 session/close 的独占连接释放，以及关闭成功的末尾消息排空/关闭失败保留。
- 核心 ACP 最终回归：617 项通过，6 项 ignored；排除已单独复现的既有 path_files_are_listed_in_the_prompt_text_even_with_optional_capabilities 失败（提示词文件 CRLF/LF 差异，与本功能无关）。手工验收测试另行执行，不在默认测试中调用外部模型。
- 前端相关5个测试文件共148项通过；新增 Direct/Workflow/AUTO 菜单隔离、常驻/解除常驻、置顶独立及后台问题不受旧 prompt 终态隐藏的回归。
- 后端领域接口测试确认常驻持久化、非 Direct 拒绝、创建历史常驻偏好不启动 run；配置边界、滑动续期、已知工具保护、等待交互与常驻保护分别测试。
- 桌面续聊与首次执行共用 App.register_acp_direct_session 分类入口；没有缓存运行时时仍根据会话元数据注册 Direct 后台接收，不依赖上一轮留下的内存状态。最终相关回归23项通过，2项外部/资源手工测试默认跳过，已另行执行。
- 真实 Agent：Claude Code 2.1.293，@agentclientprotocol/claude-agent-acp@0.87.0。隔离临时目录创建一次性 CronCreate，初始 prompt 返回后，经 Gold Band 后台消费者收到 DIRECT_TIMER_WAKE_VERIFIED；测试通过，用时21.82秒。结束后关闭工作区连接、回收测试会话。
- 前端类型检查、生产构建、桌面端 cargo check 通过。浏览器验证 Direct 右键状态切换、图钉、浅色/深色与720px窄窗口；iAB不可用、Chrome连接失败后按规则使用独立 agent-browser 会话。
- 不声明穷尽所有并发交错；锁边界以新 prompt 接管与关闭前重查为准，持续回归由上述接口测试固定。共享适配器没有 close 时保守保留，不提供进程内存硬上限。

### 可重复手工测试

- 真实定时器：显式设置 GOLD_BAND_DIRECT_TIMER_TEST_PACKAGE 为待验收的包版本，GOLD_BAND_DIRECT_TIMER_TEST_PROMPT 为一次性定时任务提示（初始回合不打印标记，定时触发输出 DIRECT_TIMER_WAKE_VERIFIED），执行 cargo test -p gold-band --lib direct_real_agent_timer_after_end_turn -- --ignored --nocapture。需要本机已登录 Claude。
- 资源诊断：cargo test -p gold-band --lib direct_retention_resource_envelope -- --ignored --nocapture；创建1/8/20个保留的模拟适配器会话并读取 Windows 测试进程树私有内存，结束后断言注册表清理。该数值测量桥接开销，不代表真实 Claude 20会话的模型上下文内存。

### 资源诊断实测

Windows debug 测试进程树（包含测试宿主与模拟适配器，排除采样 PowerShell）：1会话11,411,456 bytes，8会话75,431,936 bytes，20会话185,184,256 bytes。20会话建立约9.26秒，全部清理后注册表断言通过。模拟器并不加载真实模型上下文，因此这些数据仅验证保留消费者与模拟进程的规模趋势，不用于推断20个真实Claude会话的内存上限。真实Agent高负载、多小时定时器和操作系统休眠恢复不在此次实测覆盖范围。

### 2026-10-09：后台通知现场诊断

- task-126 已确认后台正文接收及 new-message 持久化；历史记录不包含发送时的前台状态、未读合并决策或通知标题可用性，不能从最终已读状态倒推出漏通知原因。
- 沿用既有 tracing 记录后台通知决策：active-prompt、already-observed、existing-unread-episode、new-unread-episode、target-visible、missing-localized-title；Windows Toast 记录提交成功或发送错误。提交成功不代表操作系统实际展示横幅。
- 已读确认记录 project/task/event identity。通知诊断不记录正文，复用逐消息去重，避免逐 chunk 写日志。
- 接口测试固定“普通 prompt 完成已读后，后台新消息允许通知，同一消息的后续片段不重复通知”。此次只补诊断与验收，不调整通知规则，现场原因仍需复现确认。
- 自评审：不新增状态、依赖、缓存或队列；日志限于决策与已读转换，无新增历史扫描及 UI 刷新。

### 2026-10-09：普通回合未读不再阻断后台通知

- task-129 的 19:56:11 诊断明确为 existing-unread-episode；此前只有普通 prompt 完成结果，没有后台回复，且未收到该完成结果的 ACK。不能再把所有未读结果当成同一类通知合并。
- 只在最新未读事件与已有 backgroundMessageCursor 对应时合并后台通知；普通 prompt 的完成、失败、停止结果均不抑制首次后台回复。错误/停止标记仍保留，事件 ID 推进以拒绝迟到 ACK。事件 ID 生成由 attention 模块统一提供，不增加持久字段。
- RED：unread_prompt_result_does_not_suppress_first_background_reply 在修复前因未读 Completed 拦截后台回复失败。GREEN：修复后 attention 7 项通过，活动展示与已读前端测试 39 项通过。
- task-129 工具分别在 19:56:04、19:56:09 同秒启动及完成；当前已记录计数只描述完成活动，不承诺整个后台任务结束。没有证据要求为快速工具延长正在执行状态，此次不改展示规则。
- 自评审：复用现有 cursor、ACK 与事件 ID，只有常量规模比较，无新增依赖、扫描、缓存或队列。未执行真实 Agent/Windows 横幅端到端复现。

### 2026-10-09：后台活动展示在工具间隙保持动态

- 根因：后台上一 prompt 已完成，过程组又把单个工具终态当成实时展示结束，导致正文到达前提前显示已记录。
- 复用既有 live item identity：尾部过程组只要仍对应实时活动，在工具完成及工具间隙继续动态展示；后续正文自然关闭前面的过程组。存在未完成工具时显示调用描述，否则使用现有思考中文案；历史加载无 live identity，保持静态。
- 这是展示投影，不修改 prompt/session 生命周期，不合成 Finished、不触发完成通知。无新增持久状态、依赖或计时器；摘要只遍历已有有界活动明细，无新增 I/O。
- 验证：最小测试先以 live=false（预期 true）失败，修复后活动展示 34 项通过，类型检查及生产构建通过。浏览器实际组件验证调用→思考中→正文出现后已记录，历史静态；900/1400 宽度无溢出。未调用真实 Agent。

### 常驻图钉强调色（2026-10-09）

- Direct 常驻图钉使用 accent-foreground，保持倾斜实心图钉与提示。Gold Band 深色主题为亮绿、浅色为深绿；primary 是当前主题的黑白按钮色，不用于此次强调。
- 复用现有主题变量，无新增状态或依赖。组件测试 3 项、类型检查与生产构建通过；浏览器实际侧栏验证明暗色填充及窄窗口。
- 复核淘汰策略测试 4 项及工具保护、常驻保护测试 2 项通过。默认 8/6h/20；20 为可淘汰会话的容量目标，受保护会话可超出。后台展示活动不构成用户 prompt 的排队依据，适配器接纳新 prompt 的行为不由展示状态推断。

### 后台回复不重排侧栏（2026-10-10）

- 现象：常驻会话收到后台回复后出现蓝点，但相对时间与排序不变，需重新加载列表才更新。
- 根因：record_direct_background_reply 已推进 canonical lastActivityAt 与 SQLite 投影，但前端只收到 terminal-result 事件（仅含未读结果）；后台 textDelta 走 emit_acp_event_update，不读取 lifecycle，taskActivityAt 恒为空。活动时间单一写入、侧栏按 taskActivityAt 前移的设计正确，属于该送达路径实现不完整。另外原实现先发事件再落盘，事件触发的重拉可能读到旧时间。
- 修复：ConversationTerminalResultUpdatedEventVm 增加可选 taskActivityAt，后台回复先落盘再发事件并携带该时间；Turn 终态路径置空，其活动时间仍由 lifecycle session update 送达。前端抽出 projectConversationSidebarTask，会话活动与 terminal-result 两个 reducer 共用"推进时间并前移"投影，不复制排序逻辑。
- 评审：只加一个可选字段并抽取既有逻辑；每条后台消息原本就落盘一次，无新增 I/O、轮询或状态。
- RED：terminal-result 带 taskActivityAt 时应前移，修复前顺序保持 [task-001, task-002] 失败。GREEN：该测试及"无更新时间不重排"测试通过，侧栏与 terminal-result 前端测试 58 项、相关 Rust 测试 85 项（含新增事件序列化测试）通过。

### 常驻图钉造型与静态渐变（2026-10-10）

- 造型由 lucide Pin（小尺寸下像哑铃）换为 Fluent UI System Icons 的 pin-24-filled 斜放实心图钉（MIT），只拷入单个 SVG 路径，不新增图标库依赖；置顶按钮仍用 lucide Pin，常驻与置顶在造型上区分。
- 着色由 accent-foreground 纯色改为沿钉头到针尖的静态渐变，集中定义为 RESIDENT_PIN_GRADIENT_STOPS：色相取自当前主题 emphasis → running → accent-foreground，各主题自动适配，不新增主题 token，也不按主题特判。
- 以 running 蓝为主色沿长轴分布：钉头混入 25% emphasis 成靛蓝，中段 running 混 40% 白形成金属高光，针尖混入 60% accent-foreground 成青色。仅主题三色沿对角线时 12px 下层次不足；加入粉紫会偏离蓝色主调，全彩虹或纯金色板与主题脱节，均未采用。
- 不使用流动动画：常驻是长期偏好而非待处理信号，动画会持续重绘并与未读圆点争夺注意力。每个图钉内联一个 useId 隔离的 linearGradient，仅初次绘制，无持续开销。
- 渐变使用 userSpaceOnUse 图标坐标系；objectBoundingBox 下零宽线段没有包围盒，会整段不绘制。
- 组件测试固定常驻图钉 fill 引用渐变、userSpaceOnUse、停靠点及三个主题 token 均被引用。

### 2026-10-09：后台子 Agent、过程区及停止能力补齐

- task-050 的 end_turn 后出现 Agent launch 与持续子工具事件；旧索引用父 prompt 的 completed 推断新子 Agent interrupted。现在后台启动及其后代沿用现有 goldBandBackground 来源标记，忽略旧 prompt 终态，正式 Agent 结果及失败/取消事实仍优先；全文重建与索引投影使用一致判断。
- task-051 的实时工具标识被 sessionInfo 等元数据或重复 completed 快照清除。元数据保留实时目标；重复旧终态不结束后台过程组，新的终态转换仍收敛。单工具完成不等于过程组结束，正文到达关闭前面的过程组。
- 停止按钮投影由保留 Direct runtime 提供 sessionId、connectionGeneration、activeTools、expiresAtMs；复用 active_tools，只新增最近后台活动时间。正文/思考/工具有效活动刷新窗口，usage/配置不刷新。activeTools>0 时持续显示；否则按 configs/app-config.toml 的 acpDirectBackgroundStopGraceSecs（10 秒）隐藏。普通用户 prompt 不受此展示窗口约束。
- 前端单一可清理计时器只负责可见性，切换会话释放；控制数据随现有事件和摘要传递，不轮询、不扫描历史。后台取消不设置用户 turn 为取消中，不伪造 Finished/已结束，也不改变排队、通知及累计上报。
- 后台停止携带当前会话连接 generation 和预期上一 turnId；后端持有既有 prompt 调度锁并检查活跃/接纳状态和身份后发送 session/cancel。新 prompt 已接管、旧连接或旧 turn 时无操作；前端请求期间合并重复点击，响应不清空用户新输入或新 turn 状态。协议不保证 session/cancel 删除定时器。
- RED：后台 Agent 被判 interrupted；metadata 将 live tool target 清为 null。GREEN：分支测试 33 项、Direct 相关测试 58 项（2 项真实 Agent/资源实验忽略）、前端活动/停止/状态测试 112 项及索引测试 26 项通过。模拟适配器验证无 prompt 时可发送取消、旧后台取消不能取消新 prompt；浏览器实际组件验证工具保护、10 秒隐藏、发送仍可用、正文关闭过程组和窄窗口。
- 自评审：新控制投影补足“无用户 prompt 但可取消保留会话”的能力边界，不增加业务忙闲生命周期或持久记录。每次控制读取为注册表查找、常量规模时间运算及工具表长度；无新增磁盘/网络读取。未执行真实 Claude 定时子 Agent 的端到端重跑。

### 2026-10-09：后台过程组、停止按钮与后台子 Agent 停止收敛

- 根因 1（设计缺陷）：后台过程组的动态状态取自 Markdown 流式动画目标 streamingMarkdownItemKey，该临时指针会被重放、追赶最新、滚动离开底部等路径清除；后台控制投影已存在却只驱动停止按钮。现在末尾过程组 live = 回合活跃 || 后台停止按钮可见，二者同源；batchAcpActivities 不再接收 live 指针，动画目标只服务流式渲染。现场具体是哪条路径清除了指针未复现，修复后不再依赖该指针。
- 根因 2：后台停止被接受后控制投影仍返回，取消引起的工具 cancelled/failed 终态还会续期 10 秒窗口。DirectRuntime 增加已停止标记：接受后控制投影为空；工具终态与 usage 不清除；新的正文/思考/工具启动清除并重新打开窗口。active_tools 保持真实，不影响回收保护。前端收到 accepted 立即清空当前作用域的控制投影；no-op 保持原状。
- 根因 3：后台子 Agent 没有所属用户回合，而后台停止只是无回包的 session/cancel，task-060 现场停止后 Claude 未发出任何帧（无子 Agent 结果、无工具终态），因此始终显示运行中。被接受的后台停止现在由 Direct 消费线程在停止前已接收的路由帧（route watermark）全部消费后写入隐藏的根时间线事件（source=goldBandBackgroundCancel，kind=rawDiagnostic）。时间线 locator 增加 backgroundCancel 标记（serde 默认值，旧索引无需迁移），分支投影提供最近一次后台停止。后台启动、无完成证据、停止序号不小于其启动与最新活动的子 Agent 判为 interrupted，结束时间取停止时间；停止后仍有活动则保持 running。全文重建与索引投影一致。桌面端收到该事件即复用权限/问题的会话快照刷新路径，子 Agent 状态即时更新。
- task-059 诊断：后台分析进行中发送 hi，Claude（promptQueueing）把用户 prompt 并入正在执行的后台回合，回复"稍后给出总结"后 end_turn；一次性定时任务已消耗，Agent 不会自主继续。Gold Band 的回合归属与快照正确，后台活动与新消息的合并/排队由适配器决定。
- RED：后台过程组在 live 指针被清除而控制有效时 live=false；accepted 后停止按钮仍可见；accepted 后后端仍返回控制投影；后台子 Agent 停止后仍为 running。GREEN：上述测试转绿；acp 模块 623 项通过（唯一失败为已记录的提示词 CRLF 既有问题），Direct/分支/后台相关 102 项通过，前端相关 146 项及停止流程 2 项通过，构建类型检查、生产构建、桌面端 cargo check 通过。
- 自评审：只新增一个原子标记、一个待记录水位和一种隐藏时间线事实，复用既有 seq、索引、route watermark 与会话快照刷新；无新状态机、持久字段迁移或轮询。性能：停止时追加一条事件并刷新一次会话快照；状态计算为常数次序号比较，索引投影多一次 locator 遍历（与 prompt_turns 同量级）。
- 未验证：真实 Claude 定时子 Agent 的停止后端到端重跑、浏览器实测（本轮 E 盘空间耗尽，cargo 产物改用 F 盘）。session/cancel 是否真正终止 Claude 异步子 Agent 不由协议保证。

### 2026-10-10：侧栏后台活动指示与统一后台活动判断

- 根因：侧栏活动只来自用户回合（后端 prompt_activity_under；前端 lifecycle 空闲即清空），后台事实 backgroundControl 未被侧栏使用，后台运行时侧栏无呼吸态。
- 统一判断：后端只提供 DirectBackgroundControl 事实；前端 isDirectBackgroundActive / useDirectBackgroundActive 是唯一判断，停止按钮、过程组、侧栏共用；isConversationTaskActive / useConversationTaskActive 是唯一的"会话活动"判断。后端 conversation_task_live_activity 统一服务侧栏列表加载与实时事件，新增 direct_background_control_under 按任务目录读取保留会话事实（只遍历内存注册表）。
- 侧栏：活动时图标呼吸，相对时间替换为 AcpProcessingSpinner（aria-label 复用 status.running），宽限窗口到期或后台停止被接受后恢复时间。排序规则不变。
- 通知诊断：task-061/062 在后台出现回复正文前即停止，按"后台回复正文才通知"规则不通知；task-063 后台回复正文后通知正常提交并出现"有新消息"蓝点。通知规则不变。
- RED：空闲 lifecycle 带后台事实时活动为 null；侧栏行活动时无转圈。GREEN：前端相关 8 个文件 152 项、Rust Direct/后台 73 项、桌面端侧栏/活动 30 项通过，构建类型检查仅余他人进行中改动的 useId 未导入错误。
- 自评审：仅为既有活动 VM 增加可选事实字段，复用后台控制、计时 hook 与推送链路；计时器只为活动中的后台行创建，后端查询为常数规模内存遍历。未做浏览器实测。

### 2026-10-10：Direct / Workflow 统一回收与会话头部常驻图钉

- 根因：新版 Direct 仅在超过 8 个存活会话后才按 6 小时空闲淘汰，日常使用中空闲 Direct 实际从不回收；每个 Claude 会话独占一个 claude.exe（提交内存约 400～500 MB），累积导致应用进程树内存持续增长。Workflow 早已按 600 秒 / 8 个空闲回收，两套策略并存且 Direct 缺少可达的回收路径。
- 修复：Direct 与 Workflow/AUTO 共用 retention_selection（同一 TTL、同一空闲上限、LRU 溢出），Direct 仅额外声明自身保护条件；删除 acpDirectSessionRetention 及其校验，原维护线程合并为按需启动的单一 sweep。长期存活需求由用户显式常驻表达，不再用“数量阈值内一律常驻”隐式兜底。
- 会话头部新增 ResidentPinToggle，与侧栏右键共用 App.set_conversation_resident 与侧栏会话行状态；图钉组件抽到 ResidentPin.tsx 供侧栏与头部共用。
- asyncTasks 不接入：claude-agent-acp 仅在 _meta.jetbrains.air 客户端模式下上报后台任务，该模式整体切换适配器行为（跟踪 claude-agent-acp#1285），在其进入标准 ACP 前由手动常驻覆盖定时任务场景。
- 验证：cargo test retention / direct / config::tests 168 项通过；前端 resident-pin-toggle（关/开图标与点击方向、请求中禁用防重入）与侧栏常驻、活动指示共 9 项通过；web 全量测试失败集合与 HEAD 一致（均为既有失败）；tsc 与生产构建通过。浏览器（web:dev 浏览器 API，Direct 夹具）实测浅色/深色、640px 窄宽：描边→加载→渐变实心，侧栏同步出现图钉，解除后两处同步消失，Tooltip 文案正确。
- 自评审：未新增状态或持久字段，只删除 Direct 专属配置和第二条维护线程；选择函数单次 O(N log N)，N 为存活会话数；前端仅在已加载的侧栏行上派生 props，不新增请求。

### 2026-10-13：交互归属修正、后台停止结算与取消语义

- 根因一（归属）：`handle_permission_request` / `handle_elicitation_request` 先判 `direct.is_some()` 再看活动 prompt turn，Direct 会话内 turn 进行中到达的权限/提问被误标为 session-only identity，前端 `shouldHidePendingAcpInteractions` 因此在停止/turn 终止时不隐藏卡片。
- 根因二（实时回滚）：`applyEventUpdates` 先投影后 reconcile，`shouldPreservePendingInteractions` 在 next.events 缺少结算证据时把实时 `cancelled` 权限结算回滚成 pending，卡片重进会话才消失。
- 根因三（停止路由）：`stopSession` 以 `!composerState.canStop` 决定后台路径，而 canStop 含 waitingForUserInteraction；定时唤醒后的后台提问挂起时停止被送入前台路径，后端无前台 turn 返回 no-op，界面无任何反应（runtime.log 10:48:52–10:49:01 四次 `status=no-op`）。
- 根因四（后台停止不结算）：后台提问在 Direct 消费者线程内同步阻塞等待，退出条件 `is_prompt_cancel_requested` 不含后台停止；即使路由正确，提问请求永远无人回复、消费者线程卡死、停止标记无法落盘。
- 修复：交互归属改为活动 prompt turn 优先（Direct 同时携带 sessionId，`for_turn_and_session`），仅无 turn 时 session-only；权限结算决策事件强制重申交互 identity；DirectRuntime 新增单调 background cancel epoch，等待闭包按帧到达时捕获的 epoch 检测后台停止并结算；提问响应枚举新增 Cancel，取消/超时/孤儿/批量关闭统一回 cancel，decline 仅留给用户拒绝；前端停止路由按归属判定（`routesAcpStopToBackgroundControl`），实时批投影抽为 `projectLiveAcpEventBatch`（先 reconcile 后投影）。
- RED：`direct_in_turn_interactions_are_bound_to_the_active_prompt_turn`（identity.turn_id 为 None）、`background_stop_settles_a_pending_session_interaction`（accepted 后无 action=cancel 响应）、`cancelled_elicitation_wait_reports_cancel_action`（回 decline）、前端 `keeps a live permission settlement after the display reconcile`（reconcile 后 pending 复活）、`routes Stop through the background control path`（backgroundControl 参数为 undefined）。GREEN：上述全部转绿；三个 Direct fixture 测试用共享互斥串行化并以唯一 task id 隔离 teardown（`close_direct_conversation` 按 project/task 圈定，误伤并行测试的会话）。
- 验证：Rust 全量 1623 项通过（失败集合与 HEAD 基线一致：catalog/quotes/path_files 等既有失败，另有个别负载敏感项单跑稳定通过）；前端全量 2823 项通过，失败集合与基线一致（含他人进行中改动的失败），新增 `acp-stop-routing` DOM 测试覆盖后台/前台双路径。
- 自评审：新增一个原子计数器与一个枚举值，未新增状态机、持久字段或轮询；等待循环每 200ms 多读一次原子变量；前端投影顺序调整不改变计算量。未验证：真实 Claude 定时唤醒场景的手工端到端重跑。
- 复审补正：路由初版仅以后端 acpActive / runtimeActive 判定归属，遗漏本地已提交未被 lifecycle 确认的新 prompt（sending / 乐观提示 / 等待响应），后台窗口内此时点停止会误走后台路径、前台 prompt 不被停止。改为 composer state 统一导出 `foregroundTurnOwned`（后端活动或本地在途），路由只读该事实；纯函数测试 RED（3 项 expected true to be false）→ GREEN。桌面 crate `gold-band-desktop` cargo check 通过；`ElicitationAction::Cancel` 的消费方（timeline 响应事件、前端 answered 判定、IM/intervention 校验）均按“任一响应即结算”处理，无需改动；无人值守 `settle_elicitation_without_responder` 保持 decline（策略性拒绝而非停止）。

## 2026-10-13（二）：后台活动态不消失——工具生命周期归属缺失

- 现象：task-154 设置定时任务后侧边栏持续显示活动中、停止按钮常驻。10 秒宽限期应让它消失。
- 根因（设计缺陷）：DirectRuntime.active_tools 只信任 provider 终态更新移除条目；Claude 适配器在会话被取消后不补发被停工具（AskUserQuestion …f2e036）的终态，该条目永久滞留。active_tools>0 时 isDirectBackgroundActive 恒真，宽限期规则失效；新 prompt 的工具活动重置 background_cancel_requested 后窗口重开、滞留工具使 activeTools 恒为 1；protected() 同源也使保留会话永不被回收。时间线投影早已按"停止后无完成证据即中断"结算，运行时工具集合缺同一条归属规则，两处事实不一致。
- 修复：工具集合条目记录归属（ActiveToolOwner::ForegroundTurn / BackgroundEpisode，登记时取帧的 goldBandBackground 事实，observe_update 显式收 background 参数，不再反读 _meta 投影注释）；两个边界结算——run_prompt 任意 stopReason 返回（含失败路径）结算前台工具，cancel_retained_direct_session accepted 结算后台 episode 工具；迟终态忽略不重开窗口，同 id 新 pending/in_progress 按新活动登记。
- RED：`background_stop_settles_a_pending_session_interaction` 追加断言（accepted 停止后 active_tool_count=1）、`direct_turn_end_settles_foreground_tools_without_terminal_updates`（end_turn 后 active_tool_count=1），均按根因失败。GREEN：两者转绿；新增状态级测试 `owning_boundaries_settle_only_their_own_running_tools`（归属隔离 + 迟终态不重开窗口）。
- 验证：acp 模块 630+ 通过、全量 lib 1624 通过，失败集合与既有基线一致（path_files/embedded_catalog/quotes/managed_operation 为既有失败）；clippy 触达文件无警告；gold-band-desktop cargo check 通过。未验证：真实 Claude 定时唤醒场景手工重跑。
- 自评审：仅内存 map 值类型加一个归属枚举与两个结算调用，无新增状态机/持久字段/轮询；结算是 O(工具数) 一次 retain，无锁持有跨 IO。环境备注：E 盘满导致编译失败，清理 target/debug/incremental（12G 再生缓存）恢复。
