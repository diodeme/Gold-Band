# 定时任务交互设计

## 1. 入口

左侧导航在 Agent 管理、上下文管理、运行模式管理同一组中增加：

- `AlarmClock` 图标
- `定时任务`

不增加命令栏或终端式入口。

## 2. Composer 创建流程

提交定时任务前沿用当前模式的 Agent、Workflow 和附件校验；校验失败时在 Composer 内显示具体原因，不提交空定义。配置对象以扁平 `kind` 标签传递，确保单次、重复、每隔和 Cron 使用同一条创建链路。

实现版式与原型保持一致：普通态使用发送按钮右侧的 ChevronDown 菜单进入定时创建态；定时创建态只显示 AlarmClock 创建按钮和 Settings 齿轮，并在 Composer 上方显示计划摘要与退出按钮。配置面板采用“单次 / 重复 / Cron”三段式标签；重复面板内部选择每小时、每天、工作日、每周（星期多选）或每隔（分钟/小时）。

普通 composer 保持拆分发送按钮：

```text
[发送] [ChevronDown]
```

ChevronDown 菜单中提供 `创建定时任务`。选择后进入定时创建状态：

```text
[AlarmClock 创建定时任务] [Settings]
```

- 主按钮保存定时任务定义并清空 composer。
- 齿轮只打开定时配置，不重复显示 Agent、workspace、model、thought level、permission 等已有控制。
- 配置摘要显示在 composer 附近，带 AlarmClock 图标和关闭按钮；关闭只退出定时创建状态，不发送内容。
- 定时模式创建不会立即执行；立即执行作为管理页的独立操作创建 manual occurrence，不改变下一次计划时间。
- composer 当前正文就是定时任务 instruction；附件随创建动作复制到定时任务输入目录。

## 3. 配置内容

配置对话框只负责：

- 单次日期、时间、时区
- 重复预设：每小时、每天、工作日、每周
- 每周星期一至星期日多选
- 每隔数值和单位，单位仅分钟、小时
- Cron 自定义表达式和时区
- 队列保护开关
- Direct 的新会话 / 持续会话选择

Workflow/AUTO 隐藏 Direct session policy，并强制新会话。

配置对话框使用单一 validation result 控制保存按钮，并在对应字段下即时显示本地化错误：

- 新建时默认使用系统 IANA 时区，解析失败回退 `UTC`；编辑时将规范化 UTC `at` 按任务原时区还原为本地日期和时间；
- DST 不存在时间禁用保存；DST 重复时间显示 Earlier/Later 分段选择及两个 UTC offset，默认 Earlier；
- Cron 只接受六字段表达式；每周至少选择一天；Every 只接受正整数；
- 配置合法时提交独立 `ScheduledScheduleInput`，不在前端猜测 offset 或生成持久化 UTC `ScheduleSpec`；
- 前端校验只负责即时反馈，Rust 应用服务在持久化、复制附件和通知 coordinator 前再次权威校验。

## 4. 定时任务管理页

采用安静、紧凑的列表布局，不提供永久固定详情面板。每行显示：

- instruction 派生标题
- 模式
- 调度摘要
- 下次执行时间
- 最近一次触发状态
- 启用开关
- 立即执行和查看详情/历史入口
- 更多菜单

编辑使用抽屉或对话框。任务不设置独立名称字段；标题始终取 instruction 第一条非空行。

详情页显示 occurrence 历史、Task/Run/ACP 跳转、下次执行时间、最近错误、运行次数和重试次数。`attention_required` 状态直接提供“进入会话回答”入口。

## 5. 会话标识

- 会话页头标题旁显示 `AlarmClock`，作为定时任务主标识。
- 左侧会话行显示较小的同款图标。
- 不使用 `[定时]` 前缀或高噪声 badge。
- Direct continuous 会话的每次定时触发使用轻量 AlarmClock 分隔线。

## 7. 全局列表与刷新

- 管理页使用全局扁平列表，默认展示所有已登记工作区的定时任务。
- 顶部提供“全部工作区”和具体工作区筛选；任务行显示工作区名称，不展示 `scheduled-UUID`。
- 管理页不因调度事件自动重新加载列表；手动刷新和 CRUD 成功后只更新必要行。启停操作按任务行携带的 `projectId` 执行。
- 后台创建 Task 或新 Run 后，App 根层刷新左侧会话列表，无需手动切换页面。

## 8. CRUD

- 管理页提供创建、编辑、启停、删除、立即执行、详情/历史和手动刷新。
- 管理页不因后台调度事件自动重载；手动刷新期间保留已有列表，仅刷新图标显示进行状态。
- 创建和编辑复用会话 Composer。编辑状态恢复任务内容与配置，Direct Agent 仅只读展示。
- 启停、编辑和删除成功后局部更新列表，不重新加载整个页面。
- 删除使用确认对话框，并明确历史会话不会被删除。

## 6. 状态与反馈

- 调度定义暂停、启用、错过和最近触发状态必须可在管理列表直接判断。
- 队列保护开启时展示“已有执行未结束时跳过本次”；关闭时展示“繁忙时每 30 秒重试，最多 3 次”。
- 错过时间点显示 `missed`，不暗示系统已经补跑。
- 运行中的权限等待、AskUserQuestion 等 active 状态应阻止同一定时任务并发触发。
- permission request 结束为失败并显示可进入详情的错误；AskUserQuestion 结束为需要处理并提供恢复原 Run 的入口，不能显示为永久运行中。

唤醒或重启后的过期时间点显示为 `missed`，不自动补跑；后续时间点继续按计划执行。

## 9. 统一完善交互（2026-08-05）

- “保持系统唤醒”、完成通知和历史保留天数只在设置页提供；定时任务管理页专注任务列表、筛选与任务操作，不重复展示全局运行设置。
- 时区选择展示运行环境支持的完整 IANA 时区，默认系统时区，不再限制为少数硬编码选项。
- 详情页默认展示全部 occurrence，包括 `skipped`、`missed`、`failed` 和 `attention_required`；状态筛选只改变视图，不删除诊断记录。
- occurrence 有 Task、Run 或 ACP session 引用时提供对应跳转；需要用户回答时直接进入原问题位置。
- 完成、失败、需要处理和聚合后的错过通知复用系统通知；点击后 deep link 到最有行动价值的目标。
- 所有新增可见文案进入前端 i18n；后端只返回错误码和结构化参数。
- 后端通过 `gold-band://scheduled-notification` 只发送 `kind/projectId/scheduledTaskId/occurrenceId/error/links/missedCount`，不生成对客文案；前端按当前语言生成标题和正文后调用既有原生通知管线。
- `completion` 仅在全局完成通知开启时发送；`failed` 与 `attentionRequired` 立即发送；`missed` 按 reconcile 批次聚合；`skipped/retrying` 只进入历史。去重键为 `scheduled:{occurrenceId}:{kind}`，missed 使用批次 event ID。
- 带 `scheduled_occurrence_id` 的 lifecycle 事件由定时任务运行时拥有通知决策权：通用会话通知订阅器不得再次把 `RunCompleted`、`InterventionRequested` 或 `AcpTurnFinished` 转成 OS 通知。这样完成开关只控制定时任务的成功通知，失败与需要处理仍由定时任务策略发送，且不会出现双通道重复提醒。
- failed 与 missed 点击后进入定时任务详情；attentionRequired 和 completion 有 Task/Run 链接时进入对应 Run，否则回退定时任务详情。Windows action 与 macOS/Linux 通知复用同一 scheduled payload，不建设第二套通知状态。

### 2026-08-07 实现收口

- `ScheduledTaskVm` 只返回 typed `ScheduleSpec`、原始 IANA 时区和 RFC 3339 时间；计划、时区、最近状态与空标题均由前端按当前语言生成，不再消费后端中文展示字段。
- 详情页历史不再过滤 `skipped`、`missed`，默认显示全部状态，并提供只改变当前视图的状态筛选。
- occurrence 同时具备 Task 与 Run 链接时显示图标跳转；存在 Round/Attempt 时写入 conversation deep link，目标 Run 加载后直接选择对应 session attempt。
- `ScheduledRuntimeSettings` 只挂载在设置页，使用 shadcn/ui `Switch` 与数值 `Input` 管理保持唤醒、完成通知和 `1..=3650` 天保留期；管理页不提供第二入口。
- 时区控件使用 `Intl.supportedValuesOf('timeZone')`，并以 `@vvo/tzdb` 作为不支持该 API 时的维护型数据回退；列表去重、排序并始终包含 UTC 与系统时区。
- 窄屏（小于等于 767px）自动收起 Shell 侧栏；管理页 header 改为纵向信息区与可换行操作区，避免固定桌面侧栏或筛选工具把任务标题、开关标签压成逐字换行。
- 详情 deep link 必须在会话导航回调完成初始化后才求值页面内容；直接点击任务行和通知跳转都不得因回调暂时性死区导致 React 根节点崩溃。
- Tooltip、Dialog 等跨页面 shadcn/Radix 基础上下文由应用根部统一提供，页面只声明具体控件。详情页即使存在可跳转的 occurrence 历史，也不得因缺少局部 Provider 卸载 React 根节点；桌面验收必须覆盖“存在执行历史后从列表进入详情”的路径。

### 2026-08-10 时间输入与即时校验

- 命令 authoring 输入与查询/持久化 `ScheduleSpec` 分离，At 使用本地日期、时间、IANA 时区和 Earlier/Later 选择。
- `@js-temporal/polyfill` 负责前端 DST 状态分析，`cron-parser` 负责六字段 Cron 即时校验；Rust 领域构造器保持最终权威。
- Weekly 空选择、Every 非正整数、非法 Cron、非法时区和 DST 不存在时间均在字段下显示中英文反馈并禁用保存。
- 配置对话框在 1280×900 与 390×844 视口下不得产生横向溢出，移动端底部操作区必须完整可见；无描述正文时显式关闭 Radix `aria-describedby` 关联，避免控制台警告。
