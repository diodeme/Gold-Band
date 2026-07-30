# Gold Band 上报数据分析价值报告 V2

> 配套技术方案：[统一指标上报与价值评估方案 V2](./data-reporting-and-value-evaluation.md)、[心跳上报开发方案](../../开发计划/数据采集/心跳上报开发方案.md)、[批量指标上报开发方案](../../开发计划/数据采集/批量指标上报开发方案.md)。本文只描述业务问题、指标解释和决策边界，接口与落地细节以配套技术方案为准。

## 一、核心结论

Gold Band 同时存在 Direct、Workflow、AUTO 三种不同产品心智，不能再用一套“节点成功率”解释全部价值：

- Direct 的真实使用单位是一次 ACP 回复 turn。
- Workflow 的真实执行单位是一次 runtime run。
- AUTO 的产品交付单位是 outer run，内部 dynamic graph 和 child workflow 只用于解释过程。

V2 保留现有 heartbeat 与 metrics/batch 两个端点，建立统一事件模型后，可以稳定回答四类问题：

1. 用户是否真实使用产品。
2. Agent 回复或 runtime run 是否到达终局。
3. 终局的时间、token 和人工干预代价是多少。
4. 问题发生在 Direct turn、普通 workflow、AUTO outer runtime，还是 AUTO 内部动态节点。

这里必须明确：`RunCompleted(success)` 只表示 runtime 成功走到终局，不等于用户最终确认编码目标已经完成。因此报告使用“run 终局成功率”，不再使用容易误导的“任务成功率”。

---

## 二、三种模式的价值口径

| 维度 | Direct | Workflow | AUTO |
|---|---|---|---|
| 主统计单位 | turn | run | outer run |
| 开始事实 | prompt 被后端接受 | run 状态落盘 | outer run 状态落盘 |
| 终局事实 | completed/failed/cancelled | success/failure/killed | outer success/failure/killed |
| 过程下钻 | provider/model、usage、干预 | round/node/attempt | dynamic run/node/group/child workflow |
| “成功”含义 | Agent 完成本轮回复 | runtime 完成工作流 | outer runtime 完成动态编排 |
| 不代表 | 用户问题已解决 | 用户验收通过 | 用户业务目标最终完成 |

### Direct 的特殊边界

Direct 首轮内部虽然复用一个单 Worker run，但用户看到的是持续 Agent 对话。分析时只统计 turn，不把内部壳再算作一次 Workflow run。后续同一 ACP session 的每次 prompt 都是独立 turn，token 必须按本轮增量计算。

### Workflow/AUTO 手动追问的边界

run 完成后的手动追问属于新的 ACP turn：

- mode 仍可标记为 workflow/auto，用于解释上下文来源；
- 但不能重新打开或改变原 run outcome；
- 不进入 run 成功率分母；
- 单独进入回复完成率、耗时和 token 指标。

### AUTO 的特殊边界

AUTO 是 outer run + AI-DYNAMIC graph：

- outer run 决定产品层终局；
- dynamic worker、merge、acceptance 和 workflow invocation 解释过程；
- child workflow 是下钻事实，不能重复进入 AUTO 总 run 分母；
- outer container 与内部 billable unit 不得重复计算 token。

---

## 三、业务价值全景

| 价值维度 | 指标 | Direct | Workflow | AUTO | 支撑决策 |
|---|---|---|---|---|---|
| 活跃留存 | DAU/WAU/MAU、周活跃回访、首次观测用户次周留存、启动次数 | 是 | 是 | 是 | 使用趋势、召回、版本覆盖 |
| 执行覆盖 | started→finished 终局覆盖率 | turn | run | outer run | 数据可信度、异常丢失 |
| 交付终局 | 完成/成功/失败/取消 | 回复完成 | run 终局 | outer run 终局 | 产品基线、回归监控 |
| 产物质量 | 首次通过 | 不适用 | new round=0 | acceptance 首次成功 | workflow/AUTO 优化 |
| 效率成本 | 耗时、token 效率 | 每 turn | 每成功 run | 每成功 outer run | 容量、预算、优化排序 |
| 自动化 | 干预负担/全自动率 | 干预负担 | 全自动 run | 全自动 outer run | 权限、需求补充、人工决策优化 |
| 可靠性 | 故障、中断、kill | turn failed/cancelled | runtime pause/kill | outer + leaf 故障 | runtime 健壮性 |
| 模型分析 | provider/model 分层 | turn 维度 | node 维度 | dynamic role 维度 | routing 假设与实验 |

---

## 四、指标定义与决策价值

### 4.1 活跃与留存

heartbeat 以规范化 `userId` 作为 WB 模式的用户口径，不上传 workspace，不建设设备或 session 身份。上报只包含 appStarted 与 activity：应用启动；窗口回前台、真实点击、键盘输入，以及 Direct、Workflow、AUTO 的启动/重跑/继续等有效操作。任意成功后统一节流 15 分钟，纯后台执行不产生 heartbeat。

可计算：

```text
DAU(D) = 当天 heartbeat_daily 行数
WAU(D) = 最近 7 天 distinct userId
MAU(D) = 最近 30 天 distinct userId
DAU/MAU(D) = DAU(D) / MAU(D)
周活跃回访率(W→W+1) = 两周都活跃的 userId / W 周活跃 userId
首次观测用户(W) = 全部历史中 MIN(statDate) 落在 W 的 userId
首次观测用户次周留存(W→W+1) = 首次观测用户中 W+1 活跃人数 / W 首次观测用户数
人均应用启动次数(D) = 当天 appStartCount / DAU(D)
```

次周留存只展示 W+1 已完整结束的 cohort，观察中不进入趋势，空分母返回 null。“首次观测用户”从正式上线后的首批数据计算；存量用户少，接受其首次出现被视为新用户。

客户端版本和 OS 使用用户当天最后值，每个用户每天只进入一个版本和一个 OS 分组。userId 通过跨平台系统能力读取并按不区分大小写规范化；WB 当前用户群不存在同名冲突。

不计算账号级、设备级或 workspace 级指标，不从 heartbeat 推导在线时长或活动强度。业务价值是判断用户是否持续回来、启动频次是否变化，以及新版本是否覆盖活跃用户。
### 4.2 终局覆盖率：所有结果指标的可信度前提

```text
终局覆盖率 = 有 finished 的 eligible execution / eligible started execution
```

`execution.started` 是分母，`execution.finished` 是终局。超过观察宽限期仍无终局的 execution 记为 `unclosed`。

业务价值：区分“产品真的失败”与“数据根本没收全”。如果终局覆盖率不足，任何成功率都必须同时展示覆盖率，不能单独做决策。

### 4.3 Direct 回复能力

```text
回复完成率 = completed turn / eligible started turn
回复失败率 = failed turn / eligible started turn
用户取消率 = cancelled turn / eligible started turn
单轮 token = Σ completed turn usage / completed turn
P50/P95 回复耗时 = completed turn duration 分位数
```

可回答：

- Direct 是否能稳定完成回复；
- 哪个 Agent/provider 经常失败；
- 用户是否因响应过慢频繁取消；
- 同一 Agent 的不同模型在单轮时间和 token 上有何差异。

不能回答：用户的编码问题是否真正解决。完成一轮回复只是传输与执行终局，不是产物验收。

### 4.4 Workflow run 终局

```text
run 终局成功率 = success run / eligible started run
未达终局率 = unclosed run / eligible started run
首过率 = newRoundsOpened=0 的 success run / success run
```

可回答：

- 哪些 workflow 无法稳定走到终局；
- 哪些 workflow 虽能成功，但经常需要返工轮次；
- 新版本是否造成 run 成功率或首过率退化；
- 失败集中在哪个 nodeKey、provider 或 attempt。

“首过率”只适用于 Workflow 的 new-round 语义，不用于 Direct，也不直接套用到 AUTO。

### 4.5 AUTO outer run 与动态编排质量

产品层：

```text
outer run 终局成功率
成功 outer run token
全自动 outer run 率
未达终局率
```

动态编排层：

```text
dynamic 节点数与深度
并行度分布
workflow invocation 次数
acceptance 首次通过率
rejected proposal 率
不同 dynamicNodeKind 的故障和 token
```

AUTO 首次通过定义为：成功 outer run 中所有 acceptance unit 都在第一次 attempt 成功，并且没有 rejected proposal。

可回答：

- AUTO 是否真的比固定 Workflow 更容易闭环；
- 动态编排是否生成过多节点或过深链路；
- 成本主要来自 worker、merge、acceptance 还是 child workflow；
- acceptance 频繁返工是产物质量问题还是编排问题；
- 哪些允许调用的工作流成为 AUTO 瓶颈。

### 4.6 token 效率

所有 usage 都是本 turn/attempt 的增量，并按 `usageScopeId` 去重。

```text
Direct 单轮 token = Σ turn usage / completed turn
Workflow 成功 run token = Σ success run billable unit usage / success run
AUTO 成功 outer run token = Σ outer run billable unit usage / success outer run
```

失败执行的 token 不混入“成功执行 token”，应单列：

```text
失败沉没 token
未达终局 token
取消 turn token
```

业务价值：建立预算基线、发现成本黑洞、区分“贵但完成”和“便宜但失败”。

### 4.7 自动化程度

Workflow/AUTO：

```text
全自动率 = 无 intervention 的 eligible run / eligible started run
```

Direct 是交互式产品心智，不计算全自动率，只计算每 turn 干预负担。

按 intervention kind 下钻：

- `permissionRequested`：权限策略和信任配置；
- `elicitationRequested`：输入信息不足；
- `manualDecisionRequired`：需要人工决策；
- `runtimeAbnormal/errorBlocked`：产品故障；
- `processInterrupted`：用户或环境中断。

这些类型的产品改进方向不同，不得混成一个“干预率”。

### 4.8 可靠性

Workflow/AUTO 分开计算：

- runtime 故障率；
- error blocked 率；
- 用户/环境中断率；
- kill 率；
- pause 后恢复率；
- AUTO dynamic leaf 故障率和 outer 聚合故障率。

Direct 分开计算：

- provider/transport failed turn；
- cancelled turn；
- permission/elicitation 等待；
- P95/P99 长尾回复。

业务价值：区分产品坏了、Agent 失败、用户主动停止和等待用户输入，避免把完全不同的原因相互稀释。

### 4.9 provider/model 分析

V2 能可靠提供：

- Direct：每 turn 的实际 provider/model、完成率、耗时和 token；
- Workflow：每 node attempt 的 provider/model、终局和 token；
- AUTO：按 worker/merge/acceptance/workflow invocation 角色分层的 provider/model 数据。

但“该模型参与过成功 run”只是相关性，不是模型对成功的因果贡献。不同模型 tokenizer 和价格不同，纯 token 也不是经济成本。

因此第一阶段只称“token 效率”和“模型表现相关性”。发布“模型经济性价比”前需要：

1. 同类任务/工作流/节点角色分层；
2. 价格版本或真实账单；
3. 多模型参与归因；
4. 最好使用 routing experiment 或 A/B 分流。

---

## 五、推荐决策看板

### 5.1 全局健康

- DAU、WAU、MAU、DAU/MAU、周活跃回访率与首次观测用户次周留存；
- 三种模式使用占比；
- started 数、终局覆盖率、unclosed 率；
- 客户端版本覆盖和错误分布。

### 5.2 Direct

- turn 数、回复完成/失败/取消率；
- P50/P95 响应时间；
- 单轮 token；
- Agent/provider/model 分层；
- permission/elicitation 负担。

### 5.3 Workflow

- run 终局成功率、首过率、unclosed 率；
- 成功/失败 token；
- 全自动率；
- nodeKey/attempt 故障热力；
- workflow 与版本趋势。

### 5.4 AUTO

- outer run 终局成功率和全自动率；
- 成功 outer run token；
- dynamic node 数、深度和并行度；
- acceptance 首次通过率；
- rejected proposal 与 workflow invocation 分布；
- dynamic role 的 provider/model/token 下钻。

所有成功率看板必须同时展示终局覆盖率和样本量。

---

## 六、数据不支持的结论

V2 第一阶段不得宣称：

- Agent 回复完成等于用户任务成功；
- runtime success 等于用户验收成功；
- token 少等于实际费用低；
- 某模型参与的 run 成功率等于该模型因果成功率；
- 首次观测用户次周留存等于真实注册新用户留存；
- AUTO child workflow 成功率可以与 outer run 成功率直接相加。

这些边界不是保守措辞，而是保证数据决策不误导的必要条件。

---

## 七、落地价值排序

1. **可信执行主链**：三种模式 started/finished + durable outbox。先解决分母、终局和数据丢失。
2. **执行单元与 token**：attempt UUID、usage delta、provider/resolvedModel、AUTO 父子关系。解决成本和归因。
3. **干预与可靠性**：pause/resume/intervention。解决产品瓶颈定位。
4. **活跃与留存**：heartbeatId 去重、appStarted 启动次数、userId 日聚合、周活跃回访与完整 cohort 留存。
5. **高级模型决策**：任务分层、价格和受控实验。

一句话总结：V2 不再用节点快照勉强解释所有模式，而是让 Direct turn、Workflow run、AUTO outer/dynamic graph 各自按正确统计单位进入同一事件体系；先保证数据可信，再谈成功率、成本和模型决策。
