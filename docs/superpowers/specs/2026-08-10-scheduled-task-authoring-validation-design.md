# Gold Band 定时任务时间输入与校验设计

**日期：** 2026-08-10  
**状态：** 已确认方案，等待书面规格复核  
**范围：** Direct、Workflow、AUTO 共用的定时任务创建/编辑输入；本次验收重点覆盖 Direct

## 1. 目标与非目标

本次修复定时任务配置链路中的两个根本问题：一次性本地时间由前端通过单次 offset 猜测转换，无法正确处理 DST 不存在时间与重复时间；Tauri 命令直接反序列化为持久化 `ScheduleSpec`，绕过领域构造器，使非法 Cron、空 weekly weekdays 和零 Every 间隔可能进入应用服务。

目标如下：

- 新建任务默认使用当前系统 IANA 时区，获取失败时回退 `UTC`；
- 一次性计划以“本地日期 + 本地时间 + IANA 时区 + 重复时间选择”提交给 Rust；
- DST 不存在时间禁止保存，DST 重复时间由用户选择第一次或第二次，默认第一次；
- Cron、weekly、Every 在前端即时校验，并在应用服务边界再次权威校验；
- 后端只返回结构化错误码和参数，所有用户文案由前端 i18n 生成；
- 通过前端、Rust 领域层和 Tauri 应用服务测试固化接口契约。

本次不修改 scheduler SQLite schema，不改变已持久化 `ScheduleSpec` 的 UTC deadline 语义，不增加旧命令输入兼容层，也不修改 occurrence、lease、missed 或 Direct 执行适配器。

## 2. 开源组件与边界

优先使用成熟组件：

- 前端使用 `@js-temporal/polyfill` 解析 IANA wall-clock 时间和识别 DST 边界，不自行计算时区 offset；
- 前端使用 `cron-parser` 校验六字段 Cron，不自行拆解 Cron 语法；
- Rust 继续使用现有 `chrono`、`chrono-tz` 和 `cron` crate 作为权威解析器；
- UI 继续使用 shadcn/ui、Tailwind CSS 和 Lucide 组件，不新增自研表单基础控件。

前端校验用于即时反馈，不能替代应用服务校验。Rust 转换成功后的 `ScheduleSpec` 才能进入 `ScheduledTaskDefinition` 和 SQLite repository。

## 3. 数据模型

### 3.1 输入模型与持久化模型分离

前端和 Tauri 新增只用于 authoring command 的 `ScheduledScheduleInput` / `ScheduledScheduleInputVm`：

```text
At {
  localDate: YYYY-MM-DD,
  localTime: HH:mm,
  timezone: IANA zone,
  disambiguation: earlier | later
}
Repeat {
  preset,
  hour,
  minute,
  timezone
}
Every {
  every: { value, unit },
  anchorAt,
  timezone
}
Cron {
  expression,
  timezone
}
```

查询与持久化继续返回现有 `ScheduledScheduleSpec`：

```text
At { at: UTC RFC3339, timezone }
Repeat | Every | Cron（现有规范化结构）
```

创建和更新命令不再直接接受领域 `ScheduleSpec`。`ScheduledScheduleInputVm::try_into_schedule_spec()` 是输入进入领域模型的唯一转换入口，并分别调用 `ScheduleSpec::at_local`、`repeat`、`every_in_timezone` 和 `cron`。

### 3.2 本地时间解析

`ScheduleSpec::at_local` 接收本地日期、时间、时区和 `LocalTimeDisambiguation`：

- `LocalResult::Single`：转换为 UTC 并创建 `At`；
- `LocalResult::None`：返回 `NonexistentLocalTime`；
- `LocalResult::Ambiguous(first, second)`：`earlier` 选择 UTC 较早候选，`later` 选择 UTC 较晚候选；
- 日期、时间或时区格式非法时返回对应 `ScheduleError`。

`disambiguation` 是命令输入必填字段。前端即使判断当前时间不歧义，也显式发送 `earlier`，避免不同调用方依赖隐式默认值。

## 4. 应用服务校验

Create 和 Update 在执行会话配置校验、复制附件或写 SQLite 之前先转换 schedule input。任一 schedule 错误统一映射为：

```json
{
  "code": "SCHEDULED_VALIDATION_FAILED",
  "params": {
    "field": "schedule.at|schedule.cron|schedule.weekdays|schedule.every|schedule.timezone",
    "reason": "invalid-date|invalid-time|invalid-timezone|nonexistent-local-time|invalid-cron|empty-weekdays|invalid-every-value",
    "timezone": "..."
  }
}
```

参数只包含定位和渲染所需的结构化值，不包含中文或英文客户文案。Create、Update 共用同一个转换函数，repository 不接受尚未规范化的输入 DTO。

当前 `ScheduleSpec` 的 serde 仍用于 SQLite definition 读取和查询响应；它不再作为桌面命令写入边界。开发阶段直接删除旧命令输入消费路径，不增加 fallback。

## 5. 前端交互与即时校验

新增纯函数 authoring 模块，负责系统时区、Temporal 本地时间分析、Cron 校验以及表单级 schedule validation。`ScheduledTaskDialog` 只管理字段状态和展示结果。

- 新建配置首次打开时使用系统 IANA 时区；系统时区无效或不可用时使用 `UTC`；
- 编辑已有任务时继续使用任务保存的时区，并把 UTC `at` 格式化回该时区的本地日期与时间；
- At 日期、时间、时区变化后同步计算状态；不存在时间显示字段错误并禁用保存；
- 重复时间显示“第一次 / 第二次”分段选择，默认第一次，并展示两个候选各自的 UTC offset；
- weekly 至少选择一天；取消最后一天后显示字段错误并禁用保存；
- Every 必须是十进制正整数，不能通过 `Math.max` 静默修正空值、零、负数或小数；
- Cron 使用 `cron-parser` 即时校验六字段表达式；空值和非法表达式显示字段错误；
- 保存按钮由统一 validation result 控制，不再只检查 At 是否非空。

错误文案进入 `web/src/i18n.ts` 的中英文资源。正常状态不展示实现原理或 DST 说明；只有输入无效或时间重复时展示完成决策所需的信息。

## 6. 数据流

```text
ScheduledTaskDialog fields
  -> frontend authoring validation
  -> ScheduledScheduleInput
  -> Tauri Create/Update input VM
  -> try_into_schedule_spec()
  -> validated canonical ScheduleSpec
  -> ScheduledTaskDefinition
  -> SQLite + coordinator
```

查询和编辑反向流程保持：

```text
SQLite ScheduleSpec
  -> ScheduledTaskEditVm
  -> dialog initialConfig
  -> timezone-local authoring fields
```

## 7. 测试矩阵

### 7.1 Rust 领域测试

- 普通本地时间解析到正确 UTC；
- `America/New_York` 春季跳时的不存在时间被拒绝；
- 秋季回拨时间的 `earlier` 与 `later` 分别得到两个不同 UTC；
- 非法日期、时间和 IANA 时区被拒绝；
- 空 weekly weekdays、零 Every 和非法 Cron 被拒绝。

### 7.2 Tauri 应用服务测试

- Create 和 Update 都只接受转换后的合法 schedule；
- 非法输入返回 `SCHEDULED_VALIDATION_FAILED` 及稳定的 `field/reason` 参数；
- schedule 校验失败时不创建 SQLite job、不复制附件、不通知 coordinator；
- 合法 At 输入持久化为 UTC `at` 并保留原 IANA 时区。

### 7.3 Web 测试

- 系统时区默认值与 `UTC` fallback；
- Temporal 正常、不存在、重复三类本地时间；
- 重复时间的 earlier/later payload；
- Cron 合法/非法、weekly 空选择、Every 空/零/负数/小数；
- 所有非法状态禁用保存并使用 i18n 错误文案；
- 编辑已有 At 任务能按原时区恢复本地日期和时间。

## 8. 文档与验收

实现时同步更新：

- `docs/gold-band/产品设计文档/runtime/scheduled-task.md`；
- `docs/gold-band/产品设计文档/interaction/app/scheduled-task-management.md`；
- `docs/gold-band/开发计划/定时任务/定时任务完整设计与开发计划.md`。

完成单元测试后启动前端，deep link 到定时任务创建/编辑路径，人工验证系统时区、非法 Cron、空 weekly、非法 Every、DST 不存在时间和 DST 重复时间选择。验证结束后关闭本次启动的开发服务器。
