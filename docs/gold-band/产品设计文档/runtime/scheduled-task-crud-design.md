# 定时任务 CRUD 与 Direct 生命周期设计

## 1. 目标

定时任务管理页提供创建、查看、编辑、启停、删除和手动刷新能力。管理页不因后台调度事件主动重载，避免列表闪烁和用户操作被打断；左侧会话列表仍订阅调度事件，以便新触发的 Task/Run 及时出现。

## 2. 管理页交互

- 创建：管理页提供“创建定时任务”入口，进入会话 Composer 的定时创建状态。
- 查看：使用全局扁平列表和工作区筛选，展示任务摘要、模式、工作区、计划、下次执行和最近运行。
- 编辑：从任务行菜单进入 Composer 编辑状态，恢复 instruction、附件、执行配置和定时配置。
- 启停：接口成功后只替换目标行，不重新请求整个列表。
- 删除：使用确认对话框；只删除调度定义和定时输入快照，保留已生成的 Task、Run、Round、ACP 会话和产物。
- 刷新：右上角使用刷新图标；刷新期间保留当前列表，只在图标上显示进行状态。
- 后台事件：管理页不订阅事件刷新；App 根层继续订阅并刷新左侧会话列表。

当前正在运行的执行不因删除调度定义而被终止。删除只阻止未来触发。

## 3. 编辑边界

编辑接口使用完整、类型化的更新输入，不允许任意 JSON 字段覆盖。后端根据旧定义和新定义统一计算内容身份是否变化。

以下修改保留已有 Task 关联：

- 调度时间、时区和队列保护策略。
- model、thought level 和 permission。

以下修改清除已有 Task 关联，使下一次触发创建新 Task：

- instruction 正文。
- 附件内容。
- Workflow authoring 定义。
- AUTO goal 或 allowed workflows。
- Workflow/AUTO 的 Agent 身份、Agent 策略或可用 Agent 集合。
- workspace 或运行模式。

Direct Agent 是定时任务创建时冻结的身份，创建后不可修改。编辑界面只读展示；更新接口不接受 Agent 变更。非法变更返回结构化错误码 `scheduled-task.direct-agent-immutable`。需要更换 Direct Agent 时必须创建新的定时任务。

## 4. 内容指纹

内容指纹由规范化结构计算，包含：

- instruction 正文。
- 附件内容哈希。
- workspace 身份。
- Workflow authoring 定义，或 AUTO goal / allowed workflows。
- Workflow/AUTO 的 Agent 身份、Agent 策略和可用 Agent 集合。
- Direct Agent 身份。

model、thought level、permission 和 Direct session policy 不进入内容指纹。

编辑保存时若新旧内容指纹不同，将 `taskId` 置空；历史 Task/Run 不修改、不迁移、不删除。

## 5. Direct 执行生命周期

### 新会话

每次触发都创建新的 Task、Run、Round 和 ACP session。调度定义的 `taskId` 指向最近一次触发生成的 Task，用于队列保护和来源追踪；下一次成功触发后更新为新的 Task。

### 持续会话

首次触发创建 Task、Run、Round 和 ACP session。后续触发向同一 ACP session 发送 prompt，保持同一 Task、Run 和 Round，不创建新 Run。

若最近会话不可恢复，下一次触发创建新的 Task、Run、Round 和 ACP session，并从该会话开始继续复用。

### 会话策略切换

Direct session policy 是执行策略，不是内容身份，切换时不直接清除 `taskId`：

- 新会话切换为持续会话：优先复用最近一次有效的定时会话；没有历史或会话不可恢复时创建新的持续会话链。
- 持续会话切换为新会话：保留旧关联用于队列保护；下一次触发创建新 Task 并更新关联。
- 若切换策略时同时发生内容指纹变化，仍按内容变化规则清除关联。

## 6. Workflow/AUTO 生命周期

- 内容未变化时复用同一 Task，每次触发创建新的 Run。
- 每个 Run 冻结独立的 `workflow.snapshot.json`。
- 内容指纹变化后，下一次触发创建新 Task；历史 Task/Run 保留。
- Workflow/AUTO 只允许新会话，不支持持续会话。

## 7. 接口与错误

后端提供：

- 获取单个定时任务编辑数据。
- 更新定时任务。
- 删除定时任务。
- 现有创建、列表和启停接口继续保留。

接口失败返回结构化错误码和参数，不返回对客文案。前端负责本地化展示。除不可修改的 Direct Agent 外，还需覆盖任务不存在、工作区不存在、调度无效、任务正在更新以及附件处理失败等错误。

## 8. 验收

## 9. SQLite-only application service (2026-08-06)

The desktop scheduled-task commands use one shared `ScheduledTaskService`.
SQLite is the only active definition and occurrence authority; legacy JSON is
read only by the one-time migration loader. Create validates and persists the
definition plus an immutable input snapshot, but creates no occurrence, Task,
Run, Round, or ACP session and never calls a model. Explicit run-now is a
separate coordinator request that creates one manual occurrence immediately
without advancing `next_run_at`.

Input files are copied into a job-specific unique staging directory and then
atomically renamed before the SQLite create transaction. A failed transaction
removes only that new job input directory. Delete atomically renames only the
job input directory to a unique tombstone, deletes the SQLite definition and
occurrences, restores the directory on database failure, and never deletes
linked Task/Run/Round/ACP history. Service failures are returned as structured
`ScheduledErrorCode + params + traceId`; customer-facing text remains in the
frontend. The current coordinator handle is deliberately narrow so the
deadline-driven coordinator can replace its internals in the next phase.

Implementation note: the desktop boundary exposes typed scheduled-task
read/update/delete/enable operations. Updates use `expectedUpdatedAt` for
optimistic conflict detection; Direct Agent identity is read-only and a
changed identity is rejected with a structured error code.

- 后台触发不会让管理页整表进入加载态或改变当前筛选。
- 手动刷新、启停、编辑和删除只更新必要状态。
- 删除定时任务后历史会话仍可访问，未来不再触发。
- Direct 新会话连续触发两次得到两个不同 Task。
- Direct 持续会话连续触发两次保持相同 Task、Run 和 Round。
- Direct Agent 在编辑界面不可修改，接口层也拒绝篡改。
- 内容指纹变化会使下一次触发创建新 Task；仅执行配置变化不会创建新 Task。
