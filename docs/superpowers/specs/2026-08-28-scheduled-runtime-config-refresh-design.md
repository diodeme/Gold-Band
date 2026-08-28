# 定时任务运行配置刷新与失败投影设计

## 背景与根因

定时任务的 SQLite definition、occurrence 与 deadline 推进均正常工作。故障来自两个实现缺口：

1. coordinator 在 workspace 注册时把 `App` 及其 `RuntimeConfig` 放进 `WorkspaceRegistration`。设置保存后，`DesktopState` 已更新最新配置并发送 `SettingsChanged`，但 coordinator 只执行 deadline reconcile，没有重建 registration，后续 occurrence 继续使用旧 Agent 配置。
2. occurrence 在 acceptance 前执行准备失败时会被可靠写为 `failed`，但 job 的 `lastTriggerAt / lastTriggerStatus / lastError` 投影没有推进。管理列表只读取 job 摘要，因此把真实失败展示成“尚未运行”。

根因分类是“正确设计但实现不完整”：SQLite occurrence 仍是 canonical lifecycle，现有 `SettingsChanged`、workspace registration 和 job runtime projection 已经能够表达所需状态，不需要增加新事实源。

## 方案比较

### 方案 A：设置变更时重建已注册 workspace（采用）

`SettingsChanged` 遍历 coordinator 已注册 workspace，通过现有 `app_for_workspace()` 读取最新 `DesktopContext.config`，构造 candidate registration，完成 reconcile 后再替换旧 registration。任何 candidate 失败时保留旧 registration，并复用现有重试机制。

优点是复用现有原子替换边界、配置读取入口和失败恢复语义；设置变更低频，不增加 deadline 热路径成本。缺点是一次设置保存需要按已注册 workspace 做一次有界 reconcile。

### 方案 B：每次 deadline 到达时重新创建 `App`

可以保证每次触发读取最新配置，但会把配置解析和 `App` 构造放入调度热路径，并使 registration 中缓存 `App` 的所有权含义变得模糊。

### 方案 C：给缓存的 `App` 增加共享可变配置

可以局部替换配置，但会引入新的锁、共享可变状态和跨模块更新协议，增加并发推理成本；现有 registration candidate 替换机制已经足够。

## 数据与接口

- canonical Agent 设置：`DesktopState.context.config`。
- coordinator 运行快照：`WorkspaceRegistration.app`，只允许由 workspace 注册/刷新流程替换。
- canonical 触发结果：`scheduled_occurrences.status/error_code/error_params`。
- 管理列表摘要：definition 中现有 `lastTriggerAt / lastTriggerStatus / lastError` runtime projection。

`SettingsChanged` 不再调用只对账 deadline 的 `reconcile_all()`，而是重新注册当前 `workspaces` key 集合。注册成功后才替换 candidate，失败时保留旧事实和 deadline。

pre-accept execution failure 的写入顺序固定为：停止 heartbeat，持久化 occurrence `failed`，解除 active guard，推进 definition 的失败投影，再通过既有 revision CAS 写回并发送局部 task update 事件。失败原因继续只存结构化 `SCHEDULED_EXECUTION_FAILED` 和参数，不新增后端对客文案。

## 验收

1. workspace 首次注册使用旧 runtime config；更新 runtime config 后发送 `SettingsChanged`，registration 中的 `App` 必须切换为新配置。
2. 设置刷新失败时旧 registration 和 deadline 保持可用，后续仍由现有 retry 重试。
3. scheduled occurrence 在 acceptance 前失败后，occurrence 为 `failed`，job 的 `lastTriggerAt` 等于该触发点，`lastTriggerStatus = failed`，`lastError = SCHEDULED_EXECUTION_FAILED`。
4. 下一次 `nextRunAt` 保持 materialization 已推进的值，不因失败投影倒退。
5. 管理页继续消费现有 VM 字段，无新增请求、轮询或全量历史读取。

## 过度设计与性能评审

不新增表、字段、aggregate、状态机、缓存、队列、锁或依赖。设置刷新复杂度为 `O(registered workspaces + enabled jobs)`，只发生在低频设置保存；deadline 触发路径不增加配置 I/O。失败投影沿用单 job revision CAS 和局部事件，不增加列表扫描、N+1 请求或页面级刷新。

