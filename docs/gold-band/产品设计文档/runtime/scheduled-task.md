# 定时任务运行时设计

## 1. 定位

定时任务是 Gold Band 的触发层，不是第四种运行模式。它可以触发现有的 Direct、Workflow 或 AUTO 执行链，最终仍然通过现有 `task -> run -> round -> attempt` 产生产物和状态。

普通“创建定时任务”动作只保存任务定义，不自动调用模型。如果用户没有执行其他操作，首次到达计划时间时调度器才物化 Task 并启动执行。创建成功后仍可随时使用“立即执行”；该显式操作会马上创建 `triggerKind = manual` 的 occurrence 并启动执行，无需等待首次计划时间，也不改变原计划的下一次执行时间。

## 2. Task / Run 生命周期

- 新的执行内容是新的 task。
- 相同内容再次执行时，Workflow/AUTO 在同一 task 下创建新的 run。
- Workflow/AUTO 的每个 run 都冻结自己的 `workflow.snapshot.json`。
- Workflow/AUTO 修改 instruction、附件、authoring 或 workspace 后，下一次触发创建新 task；历史 task 和 run 保留。Direct 编辑 instruction、附件或 session policy 时保留既有 Task 关联，由 new/continuous 策略决定下一次触发如何物化。
- 修改 model、thought level 或 permission 只改变后续执行配置，不创建新 task；修改 Workflow/AUTO 的 Agent 选择属于 authoring 变化，下一次触发创建新 task。
- Direct Agent 是 Direct 会话身份的一部分，定时任务创建后不可修改；需要更换 Agent 时创建新的定时任务。

定时任务定义自身可以在首次触发前保持 `taskId = null`。首次触发后记录物化的 task，后续按照内容指纹决定复用或创建新的 task。

## 3. 三种模式

### Direct

- `new`：每次触发创建新的 task、run 和 ACP session。
- `continuous`：首次触发创建 task/run/session；后续触发向同一 ACP session 发送新的 prompt，不创建新的 run。
- continuous 模式下 instruction 或附件变化时保留既有 Task/session chain；workspace 和 Direct Agent 创建后不可修改，需要变更时创建新的定时任务。
- continuous 会话在消息流中使用轻量的 AlarmClock 分隔线标识定时触发，不伪造普通用户即时发送时间。

### Workflow / AUTO

- 只能使用新会话。
- 内容未变化时复用 task，每次触发创建新的 run。
- 内容变化时下一次触发物化新的 task，旧 task 继续作为历史记录。

## 4. 定时定义

调度定义使用结构化数据，不把 Cron 或“每隔”拼接成不可解析的展示字符串。支持以下类型：

| 类型 | 语义 |
| --- | --- |
| `at` | 单次执行，使用明确的本地日期、时间和 IANA 时区 |
| `repeat` | 友好重复预设：每小时、每天、工作日、每周 |
| `every` | 固定间隔，只允许分钟或小时 |
| `cron` | 自定义 Cron 表达式和 IANA 时区 |

创建/更新命令使用独立的 authoring DTO，不直接接收持久化 `ScheduleSpec`：

- `At` 提交 `localDate + localTime + timezone + disambiguation`，由 Rust 领域构造器权威转换为 UTC `at`；
- `Repeat`、`Every`、`Cron` 也必须通过领域构造器校验后才能进入 `ScheduledTaskDefinition`；
- SQLite 与查询响应继续使用规范化 `ScheduleSpec`，不保存第二套 authoring 表示；
- 开发阶段删除旧的命令输入消费路径，不提供把持久化结构当写入 DTO 的兼容层。

重复预设的规则：

- 每小时是 wall-clock Cron，在每个整点执行，不等价于“每隔 1 小时”。
- 每天使用本地执行时间。
- 工作日固定为周一至周五。
- 每周允许多选周一至周日，并使用本地执行时间；例如周一、周三、周五生成 `MON,WED,FRI`。
- 每周至少选择一天；空 weekdays 在前端即时拒绝，并由 Rust 服务边界再次拒绝。
- 每隔只允许 `minutes` 和 `hours` 两个单位，不支持天或周。
- 每隔数值必须是正整数；空值、零、负数和小数不得静默修正。
- 自定义 Cron 使用六字段产品语法，前端通过 `cron-parser` 即时校验，Rust 通过 `cron` crate 权威校验。

### 固定间隔起算

`every` 使用 `anchorAt` 记录固定间隔的起算点：

- 首次启用时记录 `anchorAt`。
- 后续按 `anchorAt + N × unit` 计算，不做整点对齐。
- 暂停期间不补跑。
- 重新启用时重新记录 `anchorAt`。
- 修改间隔数值或单位时重新记录 `anchorAt`。

例如任务在 10:10 启用且配置为每隔 1 小时，执行时间为 11:10、12:10；11:25 暂停、14:00 恢复后，新的执行时间为 15:00、16:00。

## 5. 执行配置与内容指纹

定时创建使用 composer 当前已经选择的 Agent、workspace、model、thought level、permission 和 Direct session policy。配置被保存为后续 run 的执行配置，但不重复出现在定时配置对话框中。

内容指纹包含：

- instruction 正文
- 创建时复制到定时任务输入目录的附件内容
- Workflow authoring 定义，或 AUTO goal / allowed workflows
- Workflow/AUTO 的 Agent 身份、Agent 策略和可用 Agent 集合
- workspace 身份
- Direct 模式的 Direct Agent 身份

model、thought level 和 permission 不进入内容指纹。

Direct session policy 是执行策略，不进入内容指纹。新会话与持续会话互相切换时保留最近 Task 关联用于队列保护；下一次触发按照新策略决定创建新 Task 或继续最近的可恢复会话。

## 5.1 编辑约束

- 编辑 Direct 定时任务时只读展示 Agent，不提供修改入口；后端同样拒绝 Agent 变更。
- Workflow/AUTO 的 instruction、附件、authoring、Agent 或 workspace 变化后，将 `taskId` 置空，下一次触发创建新 Task。Direct 的 instruction、附件和 session policy 变化保留 `taskId`。
- Direct 与 Workflow/AUTO 之间切换时清除 `taskId`，避免跨执行模式复用不兼容的 Task 链路。
- 调度、时区、队列保护、model、thought level 或 permission 变化时保留 `taskId`。
- 删除定时任务只删除调度定义和定时输入快照，保留历史 Task、Run、Round、ACP 会话和产物。

### 5.2 Accepted execution snapshot

`ScheduledTaskDefinition` 与其中的 `ScheduledTaskContentSnapshot` 仍是可编辑的 authoring 权威；只有运行时可靠接受一次 occurrence 后，才生成不可变的 `ScheduledExecutionSnapshot`。快照保存 `acceptedAt`、定义 revision、内容 fingerprint、完整 `ScheduledTaskContentSnapshot` 和只用于展示的 `instructionSummary`。任务没有 title 字段，摘要不得作为 identity、名称或 authoring 数据使用。

摘要使用 Markdown parser 提取第一个非空块，将块内空白规范化后最多保留 120 个 Unicode 字符。生成过程确定性执行，不调用模型，也不读取额外文件。自动触发额外冻结 `scheduledAt + scheduleSummary + timezone`；三个字段由一个可选值统一管理，手动“立即执行”不得携带其中任何一个。

Occurrence 的执行链接由 `taskId + runId + roundId + nodeId + attemptId` 构成完整内部 locator。接受前允许链接不完整；接受事务必须一次性绑定完整 locator，后续编辑和重试不得覆写已接受快照。跨 UI 导航使用 `projectId + scheduledTaskId + taskId + runId + occurrenceId`，不使用摘要反查实体。

## 6. 队列保护和错过执行

队列保护默认开启，内部使用类型化策略：

- `skip_when_running`：同一定时任务已有 active 执行时，本次标记为 `skipped`。
- `retry_when_busy`：已有 active 执行时每 30 秒重试，最多 3 次，仍不可执行则标记为 `skipped`。

active 包括运行中、等待权限、等待 AskUserQuestion、等待用户恢复以及可恢复暂停状态。任一策略都不允许同一定时任务并发运行。

错过的 `at` 或重复时间点记录为 `missed`，不自动补跑；重复任务直接计算下一个未来时间点。第一版不提供错过执行策略配置。

### 6.1 用户配置与运行期交互

定时任务不预判或映射 Agent 的无人值守能力，也不要求特定 permission mode。创建和编辑必须原样保存用户当前选择的 Agent、model、permission mode 和 config options；计划触发与手动立即执行都通过现有 ACP 会话创建链路应用这份冻结配置。不得因为内置或自定义 Agent 未提供已知的 full-auto 标识而阻止定义保存或 occurrence 进入执行。

运行时仍出现 permission request 时，本次 occurrence 结束为 `failed`，错误码为
`SCHEDULED_PERMISSION_REQUIRED`。系统保留关联 Task、Run、ACP 会话和权限请求，并通过通知引导用户查看详情，不让调度器无限等待。

运行时出现 `AskUserQuestion` 时，本次 occurrence 结束为 `attention_required`，错误码为
`SCHEDULED_USER_INPUT_REQUIRED`。关联 Run 保持可恢复暂停，用户回答后继续原 Run；调度器释放 occurrence lease，不创建第二个同时等待回答的会话。后续时间点按队列策略记录 `skipped` 或有限次 `retrying`。

## 7. 时区

所有 wall-clock 调度显式保存 IANA 时区，默认使用系统当前时区，例如 `Asia/Shanghai`。`every` 仍保存时区以便统一展示和日志解释，但实际计算依赖 `anchorAt` 的绝对时间间隔。

一次性 `At` 的本地时间解析遵循以下规则：

- 系统 IANA 时区不可用或无效时，创建界面默认回退 `UTC`；
- DST 跳时导致的不存在时间禁止保存；
- DST 回拨导致的重复时间要求选择 `earlier` 或 `later`，默认 `earlier`；
- 前端使用 `@js-temporal/polyfill` 提供即时反馈，Rust `chrono-tz` 转换结果是写入边界的权威结果；
- 后端校验失败只返回错误码及 `field/reason` 等结构化参数，不返回对客文案。

时间输入的验收必须同时覆盖前端 Temporal 预校验、Rust 领域转换和 Tauri 应用服务副作用边界。真实 UI 验收负责确认系统时区默认值、字段错误、保存禁用状态及桌面/移动端布局；DST 不存在与重复时间的 UTC 映射由三层自动化测试固化，不能只依赖浏览器原生日期时间控件的手工表现。

## 全局任务管理

定时任务管理页默认加载所有已登记工作区，使用工作区筛选控制可见行，不把当前会话工作区作为查询边界。每行展示工作区名称和任务内容摘要，内部 `scheduled-UUID` 仅用于接口操作。

## 8. 状态边界

调度定义状态与 run 状态分离：

- 调度定义：`active | paused | completed | missed`
- 调度 occurrence：`pending | running | retrying | succeeded | failed | skipped | missed | attention_required`
- run 继续使用现有 `running | paused | completed` 与 `outcome` 约束。

调度器不得直接修改 run 的 canonical 状态；它只负责创建和认领 occurrence，调用现有 task/run 创建和启动接口，并根据带 occurrence ID 的真实执行完成事件回写 occurrence。

## 9. 调度可靠性边界

调度定义和 occurrence 使用独立的 SQLite scheduler store。`(scheduledTaskId, scheduledAt, triggerKind)` 必须有唯一约束；owner、lease、heartbeat 和 attempt 用于多进程认领及崩溃恢复。

每个任务使用独立计时器。应用启动和系统唤醒时显式检查过去的时间点并写入 `missed`，不通过推进游标追赶历史时间点。立即执行创建 `triggerKind = manual` 的 occurrence，不改变下一次计划时间。

occurrence 的成功只表示现有 Task/Run/ACP 链路真实结束；启动后台进程不等于完成。Occurrence 历史、Task/Run 历史和调度定义生命周期分别管理，删除调度定义不删除已物化的 Task/Run/ACP 历史。

## 10. 2026-08-05 统一完善修订

当前版本已经具备 occurrence SQLite、原子 claim、lease/heartbeat、真实执行终态、missed、run-now、详情诊断和 Direct/Workflow/AUTO 的 occurrence 关联。活跃 CRUD 已经以 SQLite 为唯一权威，运行时由每个 enabled job 一个 deadline 的 coordinator 驱动；旧 JSON 和每秒全量轮询不再是活跃消费路径。

统一完善采用以下边界：

- SQLite 是 definition 与 occurrence 的唯一权威；旧 JSON 只按 migration marker 导入一次。
- 单一 scheduler coordinator 使用每 job deadline 驱动，不再周期扫描全部工作区和任务。
- Direct/new、Direct/continuous、Workflow、AUTO 共用 timer、claim、lease、queue、missed、recovery 和 notification，仅执行适配器不同。

执行适配器由统一 `ScheduledExecutionAdapter` 接口承载，输入包含 occurrence 快照和任务定义，输出为 Task/Run/round/attempt 绑定。创建定时任务只保存定义与输入快照；只有首次到达计划时间或用户点击立即执行时才物化 Task/Run。用户回答 attention 时，先按原执行链接恢复同一 occurrence 的 claim 与 heartbeat，再写入 ACP 响应，避免恢复窗口内失去租约。
- keep-awake 是全局设置，默认关闭；仅在用户开启、跨 workspace 汇总后存在 enabled job 且应用仍运行时阻止系统自动睡眠，允许显示器关闭。实现统一使用 `keepawake 0.6.0`：Windows 使用 System Power API、macOS 使用 IOKit、Linux 使用系统 inhibit 后端；不启动外部命令，退出时释放进程级 guard。
- occurrence 历史展示 `skipped`、`missed` 等全部状态，终态记录默认保留 30 天、可配置范围 `1..=3650` 天。启动 reconcile 与终态 occurrence 后按 500 条有界批次清理并在批次间让出 Tokio；`attention_required`、非终态、活动 Run 链接及全部 Task/Run/ACP 历史不随 occurrence 清理。
- 管理页和设置页使用统一 i18n、完整 IANA 时区列表、原生通知 deep link 和现有 shadcn/ui 组件。

完整约束见 [`2026-08-05-scheduled-task-unified-runtime-design.md`](../../../superpowers/specs/2026-08-05-scheduled-task-unified-runtime-design.md)。

### 10.1 SQLite command boundary

Scheduled-task authoring commands converge on one shared application service.
Create remains definition-only and stages the immutable input snapshot before
the SQLite commit; it does not materialize execution state. Run-now remains a
separate explicit command that asks the coordinator to create and start one
manual occurrence while preserving the planned deadline. Delete uses an input
directory tombstone for rollback and deletes no Task/Run/Round/ACP history.
The legacy JSON store is migration input only, never an active read fallback or
write target.

### 10.2 Deadline coordinator

桌面进程只运行一个 scheduler coordinator，并通过 `DelayQueue` 为每个 enabled job 保存一个 wakeup。CRUD 提交、workspace 注册/移除、系统 resume 和应用退出都通过类型化命令更新 coordinator；不再周期扫描全部 workspace/job。注册和触发都会重新读取 SQLite revision 与 deadline，陈旧 timer 只重排、不创建 occurrence。

启动 reconcile 先处理 pending/retrying occurrence，再处理后续计划点。早于 `LATE_FIRE_GRACE` 的点写为 `missed`，grace 内近迟到点仍可执行。计划点只有到达 deadline 后才由事务物化；普通创建始终只保存定义。立即执行是独立 `RunNow` 命令，立即创建 manual occurrence，但不改变原计划 `next_run_at`。
