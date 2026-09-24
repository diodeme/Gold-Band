# Run 级执行计划与 AUTO 配置热更新技术方案

> 状态：已实施
> 适用范围：Workflow Run、AUTO Run、会话右侧工作区、运行模式管理页  
> 核心目标：让配置修改可以分别作用于当前 Run 的后续执行和下一次 Run，同时保持当前 Attempt 与既有历史不可变。

## 1. 背景与根因

当前系统存在四类相关数据：

1. Task authoring workflow：描述下一次 Run 使用的工作流定义和模型绑定。
2. Run 创建时冻结的 `workflow.snapshot.json`：描述既有 Run 的可执行工作流。
3. `NodeState.resolved_config`：描述某个 Attempt 实际使用的运行配置。
4. 任务 `authoring/auto.json`：该任务下一次 AUTO Run 的配置。项目运行模式只在创建任务时提供默认值，之后不再被会话里的下一次 Run 读写。

当前保存 Workflow 的入口实际更新 Task authoring。既有 Run 的 orchestrator 仍读取 Run 创建时冻结的 `workflow.snapshot.json`，因此保存后的内容只影响下一次 Run，不能让当前 Run 后续分发的新节点使用新配置。AUTO 模式则缺少与 Workflow 对等的 Run 级可执行快照和编辑语义。

这不是单个保存接口的实现缺陷，而是事实域不完整：系统缺少可独立修订、可并发控制、可审计的 **Run execution plan**。如果仅让前端复用现有保存接口或直接覆盖旧 snapshot，会混淆 authoring、当前执行事实和历史 Attempt，并产生分发竞态。

本方案新增统一的 Run 级 execution-plan canonical domain，并让 Workflow 与 AUTO 共用 revision、CAS、锁和保存目标语义。

## 2. 设计目标

### 2.1 功能目标

- Workflow 编辑可以选择：
  - 仅当前 Run；
  - 仅下一次 Run；
  - 当前 Run + 下一次 Run。
- 当前 Run 保存成功后，当前 provider invocation 和当前 Attempt 不切换；节点完成后的后继分发读取最新 execution plan。
- 下一次 Run 保存更新该任务的作者态：工作流写 `authoring/workflow.json`，AUTO 写 `authoring/auto.json`。两者都不回写项目运行模式或模板库。
- AUTO Run 在会话头部提供配置 icon，在右侧工作区 Tab 中打开现有运行模式页的 AUTO 表单，不跳转到运行模式管理页。会话里只编辑具体配置，不提供模板选择。
- AUTO 复用现有配置表单和校验，不实现第二套表单。模板库只在运行模式页。
- Current Run 和 Next Run 可以分裂编辑、分别保存，也可以解除分裂。
- 已发生节点仍存在 `session=continue` 回路时，保护 Agent/provider identity，避免用不同 Agent 继续旧 ACP session。

### 2.2 数据完整性目标

- 当前 Attempt 的 `resolved_config`、历史节点、历史 trace 不回写。
- Run execution-plan revision 与 `RunState.execution.revision` 独立。
- 保存与节点完成分发互斥，不出现同一次控制流收敛混用两个 plan revision。
- 所有写操作使用 revision/CAS；冲突不静默覆盖或自动合并。
- 双目标保存由后端复合 operation 负责，不由前端拼接两个普通请求。
- 未保存草稿和分裂状态不写入 durable Run state。

### 2.3 非目标

- 不新增运行模式；一个 Run 仍只属于 Workflow 或 AUTO。
- 不修改当前 provider invocation。
- 不修改已完成的 Node/Round/Attempt 历史。
- 不把所有未来可达节点都冻结为不可编辑。
- 不在 resume 失败时自动降级为 new session。
- 不复制 AUTO 配置页面。
- 不复用 `RunState.execution.revision` 作为 execution-plan revision。

## 3. 领域模型

## 3.1 Authoring 与 executable snapshot

系统继续区分两个领域：

- **Authoring definition**：供用户编辑并用于未来 Run，可以包含尚未解析的模板、binding ID 和作者态字段。
- **Executable execution plan**：供当前 Run 执行，完成权威 binding 注入和可执行性校验，冻结所有会影响后续分发的运行输入。

校验顺序保持：

1. authoring definition 结构校验；
2. 按稳定 ID 注入权威执行配置；
3. executable validation。

可以把一份作者态 draft 分别提交给 Current 和 Next，但不能把已经解析的 executable snapshot 原样写回 authoring。

## 3.2 Execution plan snapshot

建议持久化结构：

```text
tasks/<task-id>/runs/<run-id>/
  execution-plan/
    manifest.json
    revisions/
      plan-000001.json
      plan-000002.json
    operations/
      <operation-id>/
        request.json
        current.staged.json
        next.staged.json
        commit.json
```

`manifest.json` 至少包含：

```json
{
  "schemaVersion": 1,
  "planRevision": 2,
  "runMode": "workflow",
  "sourceAuthoringRevision": 7,
  "currentPlanFile": "revisions/plan-000002.json",
  "publishedAt": "..."
}
```

execution-plan snapshot 按 RunMode 区分 payload：

- Workflow：完整 executable workflow，以及 `WorkflowModelBindings` 注入后的执行绑定。
- AUTO：完整 executable AUTO 配置。

AUTO snapshot 必须冻结会影响动态分发的全部输入，包括：

- fixed/bootstrap/acceptance/candidate Agent；
- provider、model；
- permission mode、auto accept、config options；
- allowed workflow IDs、allowed profile IDs；
- routing prompt；
- `maxDynamicNodes`、`maxFanout`、`maxDepth`、`maxParallel`、`maxGroupDepth`、`maxWorkflowInvocations`；
- 其他参与后续动态节点创建或路由决策的配置。

## 3.3 Revision 边界

需要保留两个互不复用的 revision：

- `RunState.execution.revision`：Runtime execution phase 和异步回调 fencing。
- `execution-plan.planRevision`：Run 级执行计划的发布版本。

Task/template authoring 继续维护自己的 authoring revision。保存 Current+Next 时同时校验 plan revision 和 authoring revision。

## 3.4 Legacy snapshot 迁移

现有 Run 只有 `workflow.snapshot.json`。execution-plan store 首次读取时执行幂等迁移：

1. 如果 manifest 存在，读取 manifest 指向的 revision。
2. 如果 manifest 不存在但 legacy snapshot 存在，将其规范化后写为 plan revision 1，再原子发布 manifest。
3. migration 重试必须得到同一结果，不能生成多个有效 revision 1。
4. runtime 的新读取路径统一经过 execution-plan store，不能一部分读取 manifest、一部分继续读取旧 snapshot。

开发阶段明确切换后，`workflow.snapshot.json` 只作为 legacy migration 来源，不再作为并列事实源。

## 4. 保存目标与编辑状态

## 4.1 未分裂状态

未分裂状态默认展示 Next Run authoring，保存按钮提供三个目标：

1. 仅当前 Run；
2. 仅下一次 Run；
3. 当前 Run + 下一次 Run。

默认选择“仅下一次 Run”，并将用户最近选择保存为用户级偏好。偏好需要统一 schema、默认值和版本，不使用散落的 localStorage key。

Run 状态限制：

- `running`、`paused`：三个目标均可用。
- `completed`、`killed`：包含 Current 的目标禁用，自动使用仅下一次 Run。

保存后的收敛规则以提交完成后的 `diverged` 为准。工作流比较注入后的下一次 Run 与当前 Run 执行快照；AUTO 比较两份配置。保存结果带回这个值，编辑器不再根据提交了几个目标猜测。

- `diverged = false`：两侧事实相同，保持一份草稿。只保存一侧但内容没有产生差异时也保持未分裂。
- `diverged = true`：进入或保持两个 Tab，并停在本次写入的那一侧。仅当前成功时 Current 发布新 plan、Next 不变；仅下一次成功时 Next authoring 更新、Current 不变。
- 两侧都写入成功且比较后相同：两边 baseline 同步，回到未分裂。
- 只有一侧写入成功时仍返回部分提交结果；是否出现两个 Tab 只看写入后的 `diverged`。

如果 Current 已存在独立 plan override，未分裂页面显示明确提示，用户可以主动分裂查看两个事实域；不得静默覆盖任一侧。

## 4.2 分裂状态

同一工作区展示两个 Tab：

- **Current Run**：读取当前 Run 最新 execution-plan revision。
- **Next Run**：读取 Task/template authoring。

两个 Tab 独立维护：

- draft；
- baseline；
- plan revision 或 authoring revision；
- dirty 状态；
- validation errors；
- loading、saving、conflict 状态。

切换 Tab 前，先把当前编辑器里尚未写入该侧 draft 的内容放回离开的一侧，再把进入一侧的 draft 灌入编辑器。一侧保存成功不得覆盖另一侧未保存草稿。

保存目标根据当前 Tab 收敛：

- Current Run Tab：仅当前 Run、当前 Run + 下一次 Run。
- Next Run Tab：仅下一次 Run、当前 Run + 下一次 Run。

## 4.3 解除分裂

解除分裂使用 Sheet 进行一次简短决策：

1. 选择保留 Current 或 Next。
2. 选择的一份成为未分裂编辑器的唯一 draft 来源。
3. 按该 Tab 的保存目标继续提交。
4. 取消时保持分裂状态和两份草稿不变。

解除分裂不是隐式 durable save，也不自动合并两个图或配置。

## 4.4 Draft 生命周期

未保存 draft、分裂状态和当前 Tab 只保存在桌面应用生命周期内的 bounded cache：

- key 必须包含完整 `projectId/taskId/taskUuid/runId` locator；
- 关闭右侧工作区后可恢复；
- 应用重启后丢弃；
- 不写入 Task authoring、Run execution plan 或其他 durable workflow 文件；
- 不允许不同 project/task/run 串用草稿。
- 重新读取时，与该侧上次 baseline 相同的草稿改用新的 baseline。下一次 Run 因此跟到同一 Task 的共享作者态；当前 Run 只跟到本 Run 的 execution plan。与 baseline 不同的草稿整份保留，工作流和模型绑定不能拆开更新。

## 5. 当前 Run 生效边界

Current 保存成功后：

- 当前 provider invocation 不变；
- 当前 Attempt 不变；
- 当前 Attempt 的旧 `resolved_config` 不变；
- 已完成 Node/Round/Attempt 和 trace 不变；
- 当前节点必须保留稳定 `nodeId` 和 `nodeType`，不能删除；
- 当前节点的非身份配置可以修改；
- 当前节点出口边可以修改；
- 当前节点完成后的后继分发读取最新 plan；
- 尚未启动的 AI-DYNAMIC merge / acceptance 在创建节点时按当前 drive 的控制面重新注入 provider、模型和权限。group 上的注入结果保留任务文本和建组投影。固定策略未配置模型时，保留 group 上已经注入的模型。已经启动的调用不改写；
- 同一次 drive 里，已经开始的 Attempt 继续使用进入该 Attempt 时的 workflow，包括该 Attempt 内的 repair；
- 后继 Attempt 的 provider 调用使用本次分发临界区读到的同一份 plan，不在锁外另读一份；
- 未来重新进入该节点的新 Attempt 可以使用新配置。

普通拓扑编辑继续由完整 workflow validation 负责，包括节点新增/删除、边修改、可达性、`$end`、`$new-round`、failure/success branch 和出边合法性。

## 6. Resume Agent identity 保护

不能把所有未来可达节点的配置都冻结。特殊保护范围仅为：

1. 节点已经发生过；
2. 新工作流图中仍保留通过 `session=continue` resume 回该节点的方式；
3. 该节点的 Agent/provider identity 发生变化。

满足以上条件时阻断 Current 保存。其他非身份配置按一般 executable validation 处理。

Agent identity 至少包含能决定 ACP session 归属和兼容性的 Agent/provider 标识；model、permission 等字段是否属于 identity 由现有 Agent 注册和 session contract 的稳定 ID 规则决定，不能使用显示名称反查。

如果实际 resume 失败：

- 不自动降级为 new session；
- 返回结构化错误或将 Run 转为可恢复暂停状态；
- 由用户显式选择 retry 或 new session。

## 7. 并发与锁

## 7.1 锁模型

按完整 Run locator 建立 execution-plan `RwLock` registry。

节点完成路径持读锁，临界区覆盖：

1. 读取当前 execution plan；
2. 计算控制后继；
3. 读取后继节点执行定义；
4. 持久化 node → round → run 的控制流收敛；
5. 动态节点创建所需的最新 plan 读取。

Current 保存持写锁，临界区覆盖：

1. 提交时重新检查 CAS；
2. 写入 plan revision；
3. 原子更新 manifest/current locator；
4. 复合保存中 Current 目标的发布阶段。

锁不覆盖：

- provider 执行；
- 网络请求；
- catalog 查询；
- 前端编辑；
- 首次完整预检；
- 其他外部阻塞操作。

分发读锁优先。节点完成已进入控制流收敛时，并发写入失败并返回结构化 conflict；不能等待后按旧预检结果强行发布。

## 7.2 CAS 校验

预检不持锁。最终提交进入写锁后必须重新读取并比较：

- authoring revision；
- execution-plan revision；
- Run status；
- current round；
- current node；
- current attempt；
- 必要的 task/run identity 和当前节点 durable identity。

任何变化都返回 conflict，并要求客户端基于最新 canonical state 重新预检。不得自动覆盖或静默合并。

必须明确 execution-plan lock 与现有 attempt runtime lock 的固定获取顺序，避免锁反转。provider callback 的 execution fencing 继续使用现有 `attempt_runtime_state_lock`，不由 plan lock 取代。

## 8. 预检与保存接口

建议增加以下后端能力：

```text
get_conversation_execution_plan
preflight_conversation_execution_plan_save
save_conversation_execution_plan
save_conversation_execution_plan_targets
recover_conversation_execution_plan_operation
```

保存目标：

```text
ExecutionPlanTarget =
  | current
  | next
  | current-and-next
```

请求必须使用用途明确的 command/authoring DTO，不能直接接收完整持久化结构。完整 locator 至少包含：

```text
projectId
taskId
taskUuid
runId
```

预检返回：

- 当前 canonical revisions；
- normalized authoring 摘要；
- Current executable validation 结果；
- 受影响节点和拓扑变化摘要；
- resume identity 风险；
- 阻塞原因；
- 预期提交 locator。

`get_conversation_execution_plan` 的 view 返回当前 round、node 和 attempt。保存命令原样带回这些值，以及 plan revision、authoring revision 和 Run 状态。缺省或过期都在预检阶段返回 `current-locator-conflict`，不进入发布。

保存成功返回最新 canonical entity/revision 或 operation ID，前端必须以后端返回结果更新 baseline，不能假设表单输入就是最终状态。恢复 operation 同样用返回结果更新已提交目标的 baseline 和 revision。

## 9. 双目标复合保存

`current-and-next` 必须是一个后端复合 operation：

1. 接收一个 operation ID 和两侧期望 revision。
2. 对同一作者态 draft 分别进行 Current executable normalize/inject/validation 与 Next authoring normalization。
3. 在任何发布前完成可完成的全量预检。
4. staging 两侧输出。
5. 按固定顺序发布并记录 commit marker。
6. 返回两个目标各自的提交结果。

跨文件无法提供真正的文件系统事务时，接口必须返回结构化 partial result，不能声明整体成功。operation journal 应允许：

- 查询当前阶段；
- 幂等恢复；
- 判断已发布目标；
- 完成剩余提交或进入人工可恢复状态；
- 安全清理已经完成的 journal。

前端不得通过先调用 Current、再调用 Next 的两个普通请求模拟双目标提交。

## 10. 结构化错误

后端错误使用结构体管理，至少包含稳定 `code` 和机器可读 context，不包含对客文案。前端根据 i18n 映射错误码。

建议错误码：

```text
conversation.execution-plan.not-found
conversation.execution-plan.revision-conflict
conversation.execution-plan.authoring-conflict
conversation.execution-plan.current-locator-conflict
conversation.execution-plan.current-run-not-editable
conversation.execution-plan.current-node-identity-changed
conversation.execution-plan.current-node-removed
conversation.execution-plan.agent-identity-changed
conversation.execution-plan.continue-unsupported
conversation.execution-plan.resume-failed
conversation.execution-plan.validation-failed
conversation.execution-plan.partial-commit
conversation.execution-plan.recovery-required
```

Conflict 和 partial commit 必须保留本地 draft，并提供重新加载、重新预检或恢复 operation 的明确动作。校验失败和 Agent identity 冲突也保留本地 draft，但不提供重新加载：再次保存就是重新校验。Agent identity 错误必须带上 `nodeId`。校验原因已经在配置面板里；只有画布 / 配置面板收成标签时才提供「查看原因」。草稿相对已持久化基线有改动时，保存旁提供「还原」。停止或继续会话不得重载已打开的编辑工作流。

## 11. 前端交互

## 11.1 Workflow Run

Workflow Run 继续在会话右侧工作区打开 `WorkflowEditor`。现有 resource/controller 升级为带 Run context 的 execution-plan editor，不再把 Current 保存隐式路由到普通 Task workflow 保存。

## 11.2 AUTO Run

在 `ConversationRunHeader` 右侧 action group 增加 AUTO 配置 icon：

- 仅 `runMode === auto` 且非只读时显示；
- Direct 不显示；
- 使用现有 shadcn Button、Tooltip 和 `aria-label`；
- 与 Workflow view/edit 和 Rerun action 同组；
- stop/continue 继续留在 composer。

点击后在当前会话的右侧工作区打开 `auto-config` Tab，内容仍是 `RunModeManagementPage`。页面行为分两类：

- 无 Run context 的 `/chat/run-modes`：保持现有项目级运行模式和模板管理行为。
- 有 Run context：只出现在右侧 Tab 中，固定为该 Run 的 RunMode，复用现有 AUTO 表单，并启用 Current/Next、split/unsplit、preflight 和 CAS 保存。会话路由和左侧「运行模式」选中态不变。

不新建 AUTO run override 表单，不重复 catalog、模型选择器、模板和 validation 逻辑。

## 11.3 异步状态

读取、预检、保存和恢复 operation 预计超过 300ms 时，应在触发后 100ms 内显示目标位置的中间态：

- 独立工作区先打开，再加载内容；
- 已有缓存时保留内容并局部刷新；
- 写操作显示目标级 spinner；
- 保存中禁用冲突写入口；
- 重复触发需要禁用、合并或幂等；
- 失败后在当前编辑位置显示结构化原因和恢复动作。

## 12. 关键实现位置

### 12.1 后端

- `src/app/state_access.rs`
  - `load_run_workflow` 改为通过 execution-plan store 读取最新 Current revision。
  - 保持 `persist_runtime_state` 的 node → round → run 顺序。
- `src/app/orchestrator.rs`
  - Run 创建时发布初始 plan revision。
  - 节点完成、后继计算和 dynamic node 创建读取最新 plan。后继 Attempt 执行分发临界区带回的那份 workflow。
  - 读锁覆盖控制流收敛，provider 调用不持锁。
- `src/workflow_model_binding.rs`
  - 复用 authoring validation、稳定 binding 注入和 executable validation。
- `src-tauri/src/commands_conversation.rs`
  - 增加 get/preflight/save/composite/recovery command。这些 command 按请求里的 `project_id` 解析 conversation workspace，再读写该 workspace 的 execution plan；不能绑定桌面进程当前仓库。
- storage/path 模块
  - 增加 manifest、revision、operation journal 的路径和原子写入。
- runtime/DTO 模块
  - 增加 plan snapshot、manifest、target、preflight、operation result 和结构化错误类型。

### 12.2 前端

- `web/src/components/conversation/ConversationRunHeader.tsx`
  - AUTO 配置 icon、Tooltip 和回调。
- `web/src/pages/ConversationRunPage.tsx`
  - 构造完整 locator，在右侧工作区打开 Workflow 编辑和 `auto-config` 资源。AUTO 不切换会话路由。
- `web/src/pages/RunModeManagementPage.tsx`
  - 增加可选 Run context。右侧 Tab 以 `embedded` 复用现有 AUTO 表单，不渲染运行模式页标题。
- `web/src/components/workspace/right-workspace-context.tsx`
  - 增加或扩展 execution-plan edit resource 和完整 locator key。
- `web/src/components/workspace/ConversationRunWorkspaceResourcePanel.tsx`
  - 承载 Current/Next、split/unsplit 和 close resolver。
- `web/src/lib/conversation-run-cache.ts` 及 workflow editor draft 模块
  - 增加 bounded current/next/unified draft cache。
- `web/src/api.ts`、`web/src/types.ts`
  - 增加 execution-plan DTO 和 API wrapper。
- i18n
  - 增加中英文保存目标、分裂/解除分裂、预检、conflict 和恢复文案。

## 13. 实施阶段

### 阶段一：数据与存储

1. 定义 execution-plan schema、manifest、revision 和错误 DTO。
2. 增加路径和原子读写能力。
3. 实现 legacy `workflow.snapshot.json` 幂等迁移。
4. 增加 `RunExecutionPlanStore` 和按 Run locator 的锁 registry。
5. 为 migration、revision 和并发发布增加接口层测试。

### 阶段二：Runtime 接入

1. Run 创建时发布初始 execution plan。
2. 切换 `load_run_workflow` 和 orchestrator 消费路径。
3. 节点完成时在读锁内读取 plan、计算后继并持久化状态。
4. dynamic node 创建读取最新 AUTO/Workflow plan。
5. 增加 current node、resume identity 和 Attempt 不变测试。

### 阶段三：保存接口

1. 实现单目标 get/preflight/save。
2. 实现 plan、authoring、current locator 的 CAS。
3. 实现 `current-and-next` composite operation 和 journal。
4. 实现 partial result 和 recovery。
5. 暴露 Tauri command、API wrapper 和 view model。

### 阶段四：Workflow 前端

1. 升级右侧 Workflow editor resource。
2. 增加未分裂三个保存目标。
3. 增加 Current/Next 两 Tab 和独立 draft。
4. 增加自动分裂和解除分裂 Sheet。
5. 接入 preflight、conflict、partial commit 和 recovery 状态。

### 阶段五：AUTO 前端

1. 在会话头部增加 AUTO 配置 icon。
2. 为 `RunModeManagementPage` 增加可选 Run context。
3. 复用现有 AUTO 表单、模板、catalog 和 validation。
4. 接入统一 Current/Next 和 split/unsplit controller。
5. 保证无 Run context 的全局管理页行为不变。

### 阶段六：文档与验收

同步维护产品设计文档、runtime state 文档和 MVP 开发计划，完成后端、前端、并发、构建和浏览器验证。

## 14. 测试与验收

## 14.1 后端与接口测试

优先写最小失败测试，稳定复现现状下“更新 authoring 不改变当前 Run snapshot”和“AUTO 缺少 Run snapshot”的根因，再修改实现。

至少覆盖：

- legacy snapshot 幂等迁移为 revision 1；
- plan revision 与 runtime execution revision 独立；
- running/paused 允许 Current，completed/killed 拒绝 Current；
- 当前节点删除、ID/type 变化拒绝；
- 当前节点出口和非身份配置可以影响后续分发；
- 已发生节点仍可 continue 时 Agent identity 变化拒绝；
- 当前 Attempt、resolved config 和历史 trace 不回写；
- 后续新 Attempt 使用新配置；
- authoring revision、plan revision、current locator stale 均返回 conflict；
- 节点完成读锁与 plan 写锁互斥；
- provider 调用不在 plan 锁内；
- dynamic node 读取最新 plan；
- composite save、partial result、recovery 和重复恢复幂等；
- AUTO 完整可执行配置冻结且 executable 字段不污染 authoring。

## 14.2 前端测试

扩展以下测试：

- `web/tests/conversation-run-header.test.ts`
- `web/tests/run-mode-management-page.test.ts`
- `web/tests/conversation-run-page.test.ts`
- `web/tests/workflow-editor-session-draft.test.ts`
- `web/tests/workflow-auto-model-config.test.tsx`
- `web/tests/conversation-run-mode-persistence.test.ts`

至少固定：

- AUTO icon 的显示条件、Tooltip、aria-label 和完整 locator；
- 无 Run context 的运行模式管理行为不变；
- 未分裂默认 Next 和三个保存目标；
- 保存后按提交完成时的 `diverged` 决定是否分裂；内容相同的单侧保存保持未分裂，内容不同才进入两个 Tab；
- 两个 Tab 的 draft、baseline、revision 和错误互不覆盖；
- 解除分裂选择/取消语义；
- conflict 保留 draft；
- partial commit 逐目标展示；
- 工作区关闭/重开恢复应用内 draft，应用重启不恢复；
- loading、preflight、saving、success、error 的可见中间态和重复提交控制。

## 14.3 构建与实际交互验证

完成实现后执行：

- 相关 Rust 单元测试和接口测试；
- 前端单元/DOM contract 测试；
- TypeScript 类型检查；
- 前端生产构建。

使用内置浏览器 deep link 验证 Workflow Run 和 AUTO Run：

- 正常宽度；
- 窄窗口；
- 窗口重新拉宽；
- 长节点和长配置文本；
- 键盘操作；
- 浅色和深色主题；
- 当前 Run 已分裂、未分裂、completed/killed、CAS conflict 和 partial result。

验证结束后关闭测试页面并清理本次启动的进程。

## 15. 性能影响评审

### 数据规模与 I/O

- 单个 Run 的 plan revision 数量只随用户显式保存增长，不进行全项目扫描。
- 编辑器按当前 locator 和目标按需加载，不预加载完整历史 revision。
- manifest 提供 O(1) 当前 revision 定位。
- catalog 继续沿用现有按需加载和 single-flight，不因 Run context 重复全量请求。

### 内存与缓存

- draft/split 状态复用 bounded LRU，禁止无界缓存。
- 不把完整 Run 历史或所有 plan revision放入 React 全局状态。
- 页面只订阅当前 workspace resource 的 Current/Next 状态，避免扩大重渲染范围。

### 锁与阻塞

- 读锁只覆盖 plan 读取、后继计算和短文件持久化区间。
- 写锁只覆盖 CAS 与发布区间。
- provider、网络、catalog 和完整预检不持锁。
- 分发读优先可以保证节点完成不被长时间配置写入阻塞，并按产品要求让并发写入失败。

当前没有引入需要独立 benchmark 的高频算法；主要风险是锁范围和文件 I/O，一致性测试和并发接口测试应固定其边界。若后续观察到 plan revision 数量或 manifest I/O 成为瓶颈，再基于实际数据增加归档或索引，不提前设计无依据的压缩与队列。

## 14.3 本轮实现收口

- Workflow Current 保存始终执行 authoring 校验、稳定 model binding 注入和 executable 校验；不会因为 draft 看起来已经可执行而绕过权威绑定。
- AUTO Current 保存在后端复用 Workflow executable provider、permission/capability 和 allowed-workflow 校验；未知 provider 以结构化 `VALIDATION_FAILED` 拒绝。
- 普通 Task authoring 保存与 Current+Next commit 共用 authoring 写锁；锁被占用时返回 revision conflict，不再旁路覆盖 authoring。
- preflight 未通过时前端丢弃 operation ID，因为此时尚未创建 operation journal；已进入保存阶段的 operation 仍可恢复。
- dynamic proposal 在迁移完成后持有 Run plan read lock，覆盖权威 dynamic node 读取、后继物化和动态图持久化；provider invocation 仍在锁外。
- 生产读取路径统一通过 execution-plan store；`workflow.snapshot.json` 仅保留为旧 Run 的迁移来源。
- 2026-09-22 已有回归测试固定上述边界：Current binding 注入、AUTO 未知 provider、authoring lock、preflight operation ID，以及 legacy canonical 读取；前端测试若本机缺少 `@asamuzakjp/css-color` 依赖则记录为环境阻塞，不虚报通过。

## 16. 过度设计评审

新增 execution-plan aggregate、revision 和锁并非为单一 UI 竞态复制状态，而是为了表达现有模型无法表达的三个真实不变量：

1. 当前 Run 的后续分发计划可以变化，但当前 Attempt 与历史不可变；
2. Current 和 Next 属于两个不同 canonical domain；
3. 节点完成分发与计划发布必须原子互斥。

现有 `workflow.snapshot.json` 无 revision、CAS、AUTO payload 和复合保存能力，`RunState.execution.revision` 又服务于 Runtime phase fencing，无法安全复用。因此新增独立 execution-plan domain 是必要的根因修复。

方案仍优先复用现有能力：

- 复用 Workflow authoring/executable validation；
- 复用 WorkflowEditor；
- 复用 RunModeManagementPage 的 AUTO 表单；
- 复用现有原子 JSON 写入；
- 复用右侧工作区和 bounded cache；
- 复用现有 attempt runtime fencing。

不引入数据库、事件总线、后台队列、第二套 AUTO UI 或跨 Run 全局 plan 服务，新增复杂度与当前一致性需求匹配。
