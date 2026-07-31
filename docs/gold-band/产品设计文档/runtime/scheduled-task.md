# 定时任务运行时设计

## 1. 定位

定时任务是 Gold Band 的触发层，不是第四种运行模式。它可以触发现有的 Direct、Workflow 或 AUTO 执行链，最终仍然通过现有 `task -> run -> round -> attempt` 产生产物和状态。

创建定时任务只保存任务定义，不在创建动作中调用模型。首次到达执行时间时，调度器才物化 task 并启动执行。

## 2. Task / Run 生命周期

- 新的执行内容是新的 task。
- 相同内容再次执行时，Workflow/AUTO 在同一 task 下创建新的 run。
- Workflow/AUTO 的每个 run 都冻结自己的 `workflow.snapshot.json`。
- 修改 instruction、附件、Workflow 定义、AUTO 目标/允许工作流或 workspace 后，下一次触发创建新 task；历史 task 和 run 保留。
- 修改 model、thought level、permission 或 Workflow/AUTO 的 Agent 选择，只改变后续执行配置，不创建新 task。
- Direct Agent 是 Direct 会话身份的一部分，定时任务创建后不可修改；需要更换 Agent 时创建新的定时任务。

定时任务定义自身可以在首次触发前保持 `taskId = null`。首次触发后记录物化的 task，后续按照内容指纹决定复用或创建新的 task。

## 3. 三种模式

### Direct

- `new`：每次触发创建新的 task、run 和 ACP session。
- `continuous`：首次触发创建 task/run/session；后续触发向同一 ACP session 发送新的 prompt，不创建新的 run。
- continuous 模式下 instruction 或 workspace 发生变化时，下一次触发重置为新的 task/run/session；Direct Agent 创建后不可修改。
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

重复预设的规则：

- 每小时是 wall-clock Cron，在每个整点执行，不等价于“每隔 1 小时”。
- 每天使用本地执行时间。
- 工作日固定为周一至周五。
- 每周允许多选周一至周日，并使用本地执行时间；例如周一、周三、周五生成 `MON,WED,FRI`。
- 每隔只允许 `minutes` 和 `hours` 两个单位，不支持天或周。

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
- workspace 身份
- Direct 模式的 Direct Agent 身份

model、thought level、permission 和 Workflow/AUTO Agent 选择不进入内容指纹。

Direct session policy 是执行策略，不进入内容指纹。新会话与持续会话互相切换时保留最近 Task 关联用于队列保护；下一次触发按照新策略决定创建新 Task 或继续最近的可恢复会话。

## 5.1 编辑约束

- 编辑 Direct 定时任务时只读展示 Agent，不提供修改入口；后端同样拒绝 Agent 变更。
- instruction、附件、Workflow authoring、AUTO goal / allowed workflows、workspace 或运行模式变化后，将 `taskId` 置空，下一次触发创建新 Task。
- 调度、时区、队列保护、model、thought level、permission 或 Workflow/AUTO Agent 变化时保留 `taskId`。
- 删除定时任务只删除调度定义和定时输入快照，保留历史 Task、Run、Round、ACP 会话和产物。

## 6. 队列保护和错过执行

队列保护默认开启，内部使用类型化策略：

- `skip_when_running`：同一定时任务已有 active 执行时，本次标记为 `skipped`。
- `retry_when_busy`：已有 active 执行时每 30 秒重试，最多 3 次，仍不可执行则标记为 `skipped`。

active 包括运行中、等待权限、等待 AskUserQuestion、等待用户恢复以及可恢复暂停状态。任一策略都不允许同一定时任务并发运行。

错过的 `at` 或重复时间点记录为 `missed`，不自动补跑；重复任务直接计算下一个未来时间点。第一版不提供错过执行策略配置。

## 7. 时区

所有 wall-clock 调度显式保存 IANA 时区，默认使用系统当前时区，例如 `Asia/Shanghai`。`every` 仍保存时区以便统一展示和日志解释，但实际计算依赖 `anchorAt` 的绝对时间间隔。

## 全局任务管理

定时任务管理页默认加载所有已登记工作区，使用工作区筛选控制可见行，不把当前会话工作区作为查询边界。每行展示工作区名称和任务内容摘要，内部 `scheduled-UUID` 仅用于接口操作。

## 8. 状态边界

调度定义状态与 run 状态分离：

- 调度定义：`active | paused | completed | missed`
- 调度触发记录：`scheduled | running | skipped | missed | completed | failed`
- run 继续使用现有 `running | paused | completed` 与 `outcome` 约束。

调度器不得直接修改 run 的 canonical 状态；它只负责生成触发记录并调用现有 task/run 创建和启动接口。
## 当前实现边界

创建命令与 Composer 已统一使用扁平调度协议：`kind` 使用 `At`、`Repeat`、`Every`、`Cron`，字段使用 camelCase。创建前复用会话模式校验，后端校验失败会返回具体校验码并由前端展开显示。

创建入口已经位于会话 Composer 的发送按钮下拉操作中。创建时保存 instruction、模式配置、调度定义、队列保护策略、会话策略和附件快照；任务管理页负责启停。定时任务仍然由后续 scheduler loop 负责实际触发，不在创建命令中立即执行。
