# Gold Band 定时任务统一完善设计

**日期：** 2026-08-05
**状态：** 已确认，等待书面规格复核
**范围：** Direct、Workflow、AUTO 的统一调度可靠性、桌面唤醒、通知、诊断、Skill 与管理体验

## 1. 目标与非目标

定时任务是 Gold Band 的触发层，不是新的运行模式。三种模式共享调度定义、occurrence、原子领取、租约、队列保护、错过检测、通知和清理机制，但继续由各自的执行适配器维护 Task、Run、ACP session 和 authoring 语义。

本次目标是消除当前 JSON/SQLite 双数据源和每秒全量轮询两个根本性缺陷，并补齐相较 AionUi 已验证有价值的能力。不会移植 AionUi/AionCore 的 conversation domain，也不会让调度器直接修改 canonical Run 状态。

应用退出、电脑关机、强制合盖策略或企业电源策略下不承诺继续调度。本次不建设独立系统服务或云端调度器。

## 2. 开源组件与实现策略

优先复用成熟能力，不重新实现已有基础设施：

- 使用现有 SQLite repository、WAL、事务和唯一约束承担跨线程/跨进程一致性；
- 使用 Tokio timer，优先以 `tokio_util::time::DelayQueue` 保存每个 job 的独立 deadline，替代固定频率扫描；
- 使用现有 Cron、IANA 时区和 DST 计算能力，不自行解析 Cron 或维护时区规则；
- 使用现有原生 notification/intervention 管线，不建设第二套通知系统；
- 使用现有 SkillManager、shadcn/ui、Tailwind CSS、prompt-kit 和 Lucide copy-in 组件；
- keep-awake 通过窄接口调用成熟平台电源 API。若平台实现需要外部 helper，必须经 `process::background_command()` 启动。

AionCore 的 per-job timer、运行记录保留期和 AionUi 的 keep-awake/定时任务 Skill 是参考实现，但不直接移植其会话模型、CLI 环境变量协议或 `prevent-display-sleep` 默认策略。

## 3. 权威数据与迁移

### 3.1 SQLite 唯一权威

所有 create、get、list、update、enable/disable、run-now 和 delete 命令直接调用 scheduler repository。运行时不得再读写 `ScheduledTaskStore` JSON，也不得在 SQLite 失败后回退到 JSON。

`scheduled_jobs` 保存定义和派生的 `next_run_at`；`scheduled_occurrences` 保存每个计划或手动触发点。计划触发继续使用 `UNIQUE(job_id, scheduled_at, trigger_kind)` 去重。Task、Run、Round、Attempt 仍由原有领域存储管理。

### 3.2 一次性迁移

每个工作区 scheduler database 维护显式 schema/migration marker。首次升级时，在单个事务中导入旧 JSON definition/trigger；成功后写 marker。之后无论数据库是否为空，都不再次扫描 JSON。冲突返回结构化迁移错误，不静默覆盖。

旧 JSON 可保留为迁移证据，但从活跃代码路径删除读、写、同步和 fallback。删除 job 只删除调度定义、输入快照和 occurrence，不删除已经物化的 Task/Run/ACP 历史。

## 4. 调度服务

### 4.1 Coordinator

每个应用进程只有一个 `SchedulerCoordinator`，负责登记所有已注册工作区的 enabled job，并处理以下命令：

```text
RegisterWorkspace | UnregisterWorkspace
JobCreated | JobUpdated | JobEnabled | JobDisabled | JobDeleted
RunNow | Reconcile | Shutdown
```

每个 enabled job 在 DelayQueue 中只有一个未来 deadline。CRUD 事务提交成功后才向 coordinator 发送变更；发送失败时，下一次启动/reconcile 仍可从 SQLite 恢复，不回滚已提交的业务数据。

deadline 到达后，coordinator 重新读取 job 版本，计算计划点，创建或取得 occurrence 并原子 claim。旧 timer 即使在更新竞态中被唤醒，也会因 job 版本、enabled 状态和 occurrence 唯一约束而成为 no-op。

### 4.2 启动、休眠与恢复

启动时恢复过期 lease、装载 enabled job 并只安排未来时间点。系统恢复事件或 timer 在长时间休眠后唤醒时执行同一个 `reconcile(now)`：超过 late-fire grace 的历史计划点写为 `missed`，不补跑；普通 timer 抖动仍可执行原 occurrence。`late_fire_grace` 默认 60 秒，由 scheduler policy 集中配置，首版不提供用户级调节。

coordinator 不轮询 Task/Run 状态。执行中的 lease heartbeat 由 occurrence execution guard 管理；完成由带 occurrence ID 的真实生命周期事件回写。

### 4.3 队列策略

`src/scheduler/queue.rs` 是 busy 判定、重试次数和重试间隔的唯一来源。运行时不得手写 `3`、`30s` 或分叉的 active 状态判断。

`skip_when_running` 直接记录 `skipped`；`retry_when_busy` 记录有界 `retrying`，达到上限后记录 `skipped`。scheduled 与 manual occurrence 使用同一策略，run-now 不改变 `next_run_at`。

## 5. 执行适配器

统一接口接收已领取的 occurrence，并只通过原有 App/Task/Run API 启动执行：

```text
ScheduledExecutionAdapter::start(context) -> ExecutionBinding
```

`ExecutionBinding` 返回 Task/Run/Round/Attempt/session 引用，用于历史导航和生命周期关联；成功返回只表示“已启动”，不是 occurrence 成功。

- **Direct/new：** 每个 occurrence 创建新的 Task、Run 和 ACP session。
- **Direct/continuous：** 复用可恢复的 Task/session chain；每个定时 prompt 仍绑定独立 occurrence。
- **Workflow：** content fingerprint 未变化时复用 Task，每次创建新 Run；authoring 变化时新建 Task。
- **AUTO：** 保留 AUTO goal、allowed workflows、快照和现有 Task/Run 物化规则。

`RunCompleted`、`AcpTurnFinished` 和 intervention 事件携带 occurrence ID。permission request 映射为 `failed + SCHEDULED_PERMISSION_REQUIRED`；`AskUserQuestion` 映射为 `attention_required + SCHEDULED_USER_INPUT_REQUIRED` 并保留可恢复 Run。

## 6. Keep-awake

keep-awake 是全局持久化偏好，默认关闭，不是 job 字段。激活条件固定为：

```text
keep_awake_enabled && enabled_job_count > 0 && app_is_running
```

`SystemSleepInhibitor` 只提供 `acquire(reason)` 与 `release()`，平台实现和调度逻辑隔离。状态协调器必须幂等处理重复启停、应用退出、最后一个 job 停用和设置切换；异常只记录结构化诊断，不阻止 scheduler 运行。

语义为阻止系统自动睡眠或应用挂起，但允许显示器关闭。UI 明确提示能耗和能力边界，不采用 AionUi 更强的 `prevent-display-sleep` 默认行为。Direct、Workflow、AUTO 不分别持有 inhibitor。

## 7. 通知、历史与保留期

复用现有 notification/intervention 管线，新增 scheduled occurrence payload。后端只提供事件类型、错误码、参数、job ID 和 ExecutionBinding；前端/系统通知层生成本地化文案。

- `succeeded`：按全局通知偏好发送完成通知；
- `failed`、`attention_required`：默认发送，并跳转到详情或可恢复 Run/问题；
- `missed`：默认聚合，避免一次休眠产生通知风暴；
- `skipped`、`retrying`：保留在历史和诊断中，默认不发送系统通知。

详情页显示所有 occurrence 状态，不再过滤 `skipped`、`missed`。存在关联引用时提供 Task、Run、ACP session 跳转。

终态 occurrence 默认保留 30 天。后台维护任务按 repository 批量删除超过保留期的 occurrence；`attention_required` 以及仍关联未完成 Run 的记录不清理。job 的累计诊断计数和已物化 Task/Run/ACP 历史不受影响。保留天数作为范围为 1 至 3650 天的全局设置，首版 UI 只展示默认值时也不得硬编码在清理实现中。

## 8. Skill 能力

### 8.1 内置定时任务 Skill

新增资源化的 zh-CN/en 内置 Skill，两种语言目录结构一致。长文本放在 `src/prompts/<locale>/skills/scheduled-task/`，由 built-in skill registry 发布给现有 SkillManager，Rust 实现不嵌入长 prompt。

Skill 通过类型化内部 tool boundary 调用 create、list、get、update、pause、resume、run-now 和 delete，不使用 CLI、环境变量或自由格式 JSON。工具 schema 使用稳定枚举和结构化错误，按 Direct、Workflow、AUTO 校验各自必需字段。

### 8.2 沉淀为 Skill

成功 occurrence 的详情页提供“沉淀为 Skill”入口。系统基于该 occurrence 的输入、执行模式、稳定步骤和产物生成草稿，但必须展示预览并由用户确认后，才写入现有 Project/Global Skill 存储。

该流程不自动修改 scheduled job，也不自动覆盖已有 Skill。用户显式选择后，job 才保存 Skill ID/version 引用；后续版本变化可追踪。Direct、Workflow、AUTO 共享建议流程，模式适配器只提供不同的执行 provenance。

## 9. UI 与 i18n

定时任务管理页保持紧凑列表，详情页承担诊断和历史。新增或调整的可见文案全部进入 `web/src/i18n.ts`，后端不返回中文或英文客户文案。

- 时区选择使用 `Intl.supportedValuesOf("timeZone")` 获取完整 IANA 列表，并提供兼容性 fallback；默认系统时区；
- 管理页和设置页提供同一个 keep-awake Switch，显示当前是否已实际生效；
- occurrence 历史支持状态筛选，但默认不过滤 `skipped`、`missed`；
- Task/Run/session 引用使用可点击操作，`attention_required` 直接进入待回答位置；
- 使用 shadcn/ui、Tailwind CSS 和 Lucide；不新增终端式入口或自研基础控件。

## 10. 错误与可观测性

所有新增失败使用 `{ code, params, trace_id? }`，客户文案由前端 i18n 映射。至少新增或复用：

```text
SCHEDULED_MIGRATION_CONFLICT
SCHEDULED_COORDINATOR_UNAVAILABLE
SCHEDULED_POWER_INHIBITOR_FAILED
SCHEDULED_NOTIFICATION_FAILED
SCHEDULED_SKILL_VALIDATION_FAILED
```

通知失败、keep-awake 失败和清理失败不改变 occurrence 的执行结果，但写入结构化日志和诊断。claim/lease/finish 失败按所有权与错误类型决定重试，不以字符串匹配错误。

## 11. 测试与验收

实施遵循 TDD，先修复当前 scheduler 测试构造了已移除 event 字段导致的编译失败，再逐层固化接口：

1. repository：SQLite-only CRUD、一次性迁移 marker、冲突、并发 claim、lease、清理边界；
2. coordinator：paused time 下的 timer 创建/重排/取消、陈旧 timer、启动与休眠 reconcile；
3. adapters：Direct/new、Direct/continuous、Workflow、AUTO 的 Task/Run/session 规则和真实完成；
4. power：fake inhibitor 验证设置、enabled job 数和退出状态机；平台 API 只做窄集成测试；
5. notification：事件映射、去重、missed 聚合和 deep link；
6. Skill：双语资源一致、tool schema、三模式校验和确认后写入；
7. Web：完整历史、跳转、IANA 时区、keep-awake、i18n 和浏览器/桌面 facade；
8. 全量 Rust/Web 测试、生产构建，并启动前端通过 deep link 做桌面与移动宽度验证，结束后清理测试资源。

## 12. 实施顺序

1. 修复测试基线并建立 SQLite-only repository contract。
2. 删除活跃 JSON 路径，完成迁移 marker 和命令层切换。
3. 引入 coordinator/DelayQueue，统一 queue policy 和恢复语义。
4. 验证四个执行适配器（Direct 两种策略、Workflow、AUTO）。
5. 接入 keep-awake、通知、全状态历史和保留期。
6. 完成双语 Skill、沉淀为 Skill、IANA 时区和全量 i18n。
7. 完成全量自动化与可视化验收，同步产品设计和开发计划。

每一步均保持 SQLite 为唯一权威，不创建兼容层，不以旧 JSON fallback 换取局部可用性。
