# AI-DYNAMIC 节点方案

## 0. 当前实现状态（2026-06-01）

本轮已完成 V1 成功路径的可运行闭环：

- DSL 已支持 `worker | ai-dynamic`，并校验 AI-DYNAMIC fan-out provider、动态控制限制、merge/acceptance provider 与 allowed workflow 引用。
- workflow 编辑器已通过 `allowAiDynamic` 能力开关控制 AI-DYNAMIC 节点新增入口；新增按钮与默认节点名走 i18n，按钮旁提供问号说明。
- AI-DYNAMIC Inspector 已调整为“节点 ID + 四个默认收起编辑块”：基础信息、Fan-out Agent、Merge Agent、Acceptance Agent；fan-out/merge/acceptance 只配置 agent，不再由用户配置角色和目标。
- allowed workflow 选择已改为可搜索多选下拉，按可选/不可选分组展示；不可选项禁用并显示原因。默认工作流不豁免 `workflow.id` 重复限制；`workflowId` 存储 workflow DSL 内的 `workflow.id`，不再使用模板外层 `template.id`。
- runtime 已在外层 orchestrator 中识别 `NodeDsl::AiDynamic`，进入节点后创建独立 `dynamic/` 状态目录、bootstrap internal worker、proposal、group、merge、acceptance 和 completion 派生逻辑。
- prompt 目录当前按语言 + 职责组织：`src/prompts/zh-CN/profile/` 与 `src/prompts/en/profile/` 存放内置 profile，`src/prompts/zh-CN/runtime/` 与 `src/prompts/en/runtime/` 存放通用 runtime prompt（如 `system.md`、`invalid_output_repair.md`），`src/prompts/<lang>/runtime/ai-dynamic/` 存放 AI-DYNAMIC 相关 prompt（如 `system.md`、`proposal_repair.md`）；其中 system prompt 模板统一使用 minijinja 渲染，runtime 决定的 dynamic 上下文、路径、限制、历史与输出协议进入 system prompt，requirement 与当前 goal 进入 user prompt。
- `dynamic-node-completion` 已支持 `end`、`single`、`fanout`；invalid proposal 会写入 rejected proposal 并让 run 进入 error-blocked pause。
- fanout group 已支持 branch terminal 检测、merge agent、acceptance agent 和 group closed 后的 AI-DYNAMIC success；底层状态已加入 `parentGroupId`，支持多层 fanout 的父子 group 闭合关系。
- workflow invocation 已支持引用 run start 时冻结的 allowed workflow snapshot，child run 成功后由 runtime 包装 completion。
- view model 已暴露 AI-DYNAMIC summary / internal graph / groups / proposals，外层 graph 保持单个复合节点。
- 新增回归测试覆盖 fanout+merge+acceptance、非法 workflow invocation、冻结 allowed workflow snapshot；同时通过 `cargo test`、`npm run web:test`、`npm run web:build`。

V1 仍保持以下边界：不做 direct mode、triage-result、route-decision/replan、nested AI-DYNAMIC 和局部失败恢复。

## 1. 背景

当前 Gold Band 已经支持固定工作流编排，典型路径类似：

```text
plan -> dev -> review -> test -> accept -> cleanup -> end
```

固定工作流的优点是可预测、可恢复、可审计，但在以下场景会遇到限制：

1. 大任务对单个 agent 节点来说过大，需要先拆成多个子任务。
2. 子任务之间可能存在串行、并行、汇总关系。
3. 固定工作流无法根据中间执行结果动态决定下一步。
4. 大任务并行修改代码时，需要 worktree 隔离、合并和验收。

本方案不废弃固定工作流，也不把整个 run 改成自由动态模式，而是在普通工作流中新增一种特殊复合节点：`AI-DYNAMIC`。

---

## 2. 产品定位

`AI-DYNAMIC` 是一个可嵌入普通工作流的特殊复合节点。

外层工作流仍然由用户正常设计：

```text
worker -> worker -> AI-DYNAMIC -> worker -> end
```

或：

```text
AI-DYNAMIC -> accept -> cleanup -> end
```

`AI-DYNAMIC` 只负责节点内部的动态编排，不关心外部工作流长什么样。

---

## 3. 工作流编辑开关

AI dynamic 不是一种独立 workflow mode。

工作流编辑器中应提供一个能力开关：

```ts
type WorkflowEditorCapabilities = {
  allowAiDynamic: boolean;
};
```

含义：

- `allowAiDynamic = false`：工作流编辑器只能创建普通 worker 节点。
- `allowAiDynamic = true`：工作流编辑器允许新增 `AI-DYNAMIC` 节点。

因此不存在固定的 `ai-dynamic workflow template` 概念。用户可以自由设计外层工作流，`AI-DYNAMIC` 只是其中一种节点类型。

简单任务不需要新增 `direct` 模式；可以由极简普通工作流表达，例如：

```text
dev -> end
```

或：

```text
dev -> test -> end
```

---

## 4. 核心原则

### 4.1 外层确定，内层动态

外层仍然走现有 workflow DSL 和 control engine。

当 runtime 进入 `AI-DYNAMIC` 节点时，节点内部启动一套 dynamic graph。

示例：

```text
外层 workflow:
  plan -> AI-DYNAMIC -> final-accept -> cleanup -> end

AI-DYNAMIC 内部:
  bootstrap
    -> node-plan
      -> fanout group-core
         ├─ node-impl-cli
         └─ node-impl-config
      -> merge-core
      -> accept-core
    -> end
```

### 4.2 Agent 只提议，runtime 才执行

AI-DYNAMIC 内部 agent 不能直接修改 runtime 状态。

agent 只能输出固定 schema 的控制 artifact：

```text
dynamic-node-completion
```

该 artifact 可以提议：

- 当前链路结束
- 创建一个后继节点
- 创建多个后继节点，即 fanout
- 调用一个已允许的 workflow snapshot

runtime 负责：

- 校验 artifact
- 检查预算限制
- 检查依赖是否合法
- materialize 内部节点
- 创建 fanout group
- 调度 merge agent
- 调度 acceptance agent
- 判断 AI-DYNAMIC 是否完成

### 4.3 Merge 是 agent，不是 runtime 隐式操作

runtime 不直接合代码。

runtime 只在 group ready 时创建 merge agent 节点，并把上下文交给 merge agent：

- group id
- 分支节点列表
- worktree 路径
- artifact 路径
- attachment 路径
- completion summary
- main workspace 路径

merge agent 自己执行合并、解决冲突并输出结果。

### 4.4 Artifact 属于具体节点

不要设计 unit-level output。

错误示例：

```json
{
  "id": "unit-001",
  "output": { "artifact": "implementation-result" }
}
```

原因：

- 当前系统 artifact 是节点产物。
- dynamic 内部节点是运行时生成的。
- runtime 预先不知道哪个节点会输出哪个业务 artifact。
- 只有 artifact 名字不够，还需要 schema 和 success condition。

正确方式：

- 控制流 artifact 固定为 `dynamic-node-completion`。
- 业务 artifact 由具体 internal worker 或 child workflow 中的具体节点产出。
- artifact 归属到具体 node / attempt。

---

## 5. AI-DYNAMIC 外层 DSL

现有 `NodeDsl` 只有 worker，后续可扩展：

```ts
type NodeDsl = WorkerNode | AiDynamicNode;
```

`AI-DYNAMIC` 节点示例：

```json
{
  "id": "ai-dynamic",
  "type": "ai-dynamic",
  "provider": "claude-acp",
  "control": {
    "maxDynamicNodes": 20,
    "maxFanout": 5,
    "maxDepth": 6,
    "maxParallel": 3,
    "maxGroupDepth": 1,
    "maxWorkflowInvocations": 10,
    "allowNestedDynamic": false
  },
  "allowedWorkflows": [
    { "workflowId": "dev-review-test-accept" },
    { "workflowId": "review-test" }
  ],
  "merge": {
    "provider": "claude-acp"
  },
  "acceptance": {
    "provider": "claude-acp"
  }
}
```

外层 edge 仍然普通：

```json
[
  { "from": "plan", "to": "ai-dynamic", "on": "success" },
  { "from": "ai-dynamic", "to": "final-accept", "on": "success" },
  { "from": "ai-dynamic", "to": "$end", "on": "failure" }
]
```

---

## 6. `allowNestedDynamic` 的含义

```json
"allowNestedDynamic": false
```

不表示禁止 fanout。

它只禁止：

```text
AI-DYNAMIC 内部再创建另一个 AI-DYNAMIC
```

禁止这种递归：

```text
outer workflow
  -> AI-DYNAMIC A
      -> internal node
      -> AI-DYNAMIC B
          -> internal node
```

但仍然允许：

```text
AI-DYNAMIC
  -> node-1
      -> node-2
```

也允许 fanout：

```text
AI-DYNAMIC
  -> node-1
      ├─ node-2
      ├─ node-3
      └─ node-4
```

V1 建议固定：

```json
"allowNestedDynamic": false
```

原因：

- 状态机复杂度会递归膨胀。
- UI 展示会复杂。
- 预算控制会复杂。
- 失败恢复会复杂。

---

## 7. allowed workflows 与 snapshot

AI-DYNAMIC 内部可以引用已有工作流，但不能裸引用 workflow。

应建模为一种内部动态节点：

```ts
type DynamicNodeKind = 'worker' | 'workflow-invocation';
```

### 7.1 用户显式选择 allowed workflows

创建或编辑 AI-DYNAMIC 节点时，用户显式选择允许内部引用的工作流：

```json
"allowedWorkflows": [
  { "workflowId": "dev-review-test-accept" },
  { "workflowId": "review-test" }
]
```

不是默认允许全部工作流。

### 7.2 编辑时验证

保存 AI-DYNAMIC 配置时验证：

- workflow 是否存在
- workflow DSL 是否合法
- workflow 是否包含 AI-DYNAMIC
- 如果包含 AI-DYNAMIC 且 `allowNestedDynamic=false`，拒绝保存或提示用户移除

### 7.3 run start 时冻结 snapshot

启动 run 时，不能只保存 workflowId，应冻结 allowed workflow 快照：

```json
{
  "allowedWorkflowSnapshots": [
    {
      "workflowId": "dev-review-test-accept",
      "snapshotId": "wf-snapshot-001",
      "name": "开发-审查-测试-验收",
      "containsAiDynamic": false,
      "workflow": {
        "version": "0.1",
        "id": "dev-review-test-accept",
        "entry": "dev",
        "nodes": [],
        "edges": []
      }
    }
  ]
}
```

运行中 artifact 引用 workflow 时，只能引用本次 run snapshot 中的 workflow，不读取 live workflow。

### 7.4 为什么必须 snapshot

如果只保存 workflowId：

- 用户启动后修改 workflow，会导致同一个 run 前后语义不一致。
- 回放时无法还原当时执行版本。
- UI 展示的 workflow 可能不是实际执行版本。
- 内部节点执行到一半，workflow 被改掉，会破坏可恢复性。

因此规则是：

```text
编辑时校验 live workflow；run start 时冻结 snapshot；运行中只引用 snapshot。
```

---

## 8. AI-DYNAMIC 内部节点类型

V1 支持两类内部节点。

### 8.1 worker

单个 agent 节点：

```json
{
  "id": "node-plan-cli",
  "kind": "worker",
  "title": "分析 CLI 模块边界",
  "task": "分析 CLI 参数解析模块边界，给出实现范围",
  "provider": "claude-acp",
  "profile": "plan",
  "workspace": {
    "mode": "readonly"
  },
  "dependsOn": []
}
```

### 8.2 workflow-invocation

调用一个已允许的 workflow snapshot：

```json
{
  "id": "node-impl-cli",
  "kind": "workflow-invocation",
  "title": "执行 CLI 模块重构流程",
  "workflowId": "dev-review-test-accept",
  "task": "在独立 worktree 中完成 CLI 参数解析模块重构",
  "workspace": {
    "mode": "worktree"
  },
  "dependsOn": ["node-plan-cli"]
}
```

执行语义：

```text
workflow-invocation node
  -> runtime 启动 child workflow run
  -> child run success，则该 dynamic node success
  -> child run failure，则该 dynamic node failure / pause
```

---

## 9. 控制 artifact：dynamic-node-completion

每个 AI-DYNAMIC 内部节点完成时，固定输出一个控制 artifact：

```text
dynamic-node-completion
```

该 artifact 是 AI-DYNAMIC 内部控制流的唯一入口。

V1 不引入：

- triage-result
- route-decision
- 多种情况下输出不同 artifact

---

## 10. dynamic-node-completion schema

基础结构：

```json
{
  "version": "0.1",
  "kind": "dynamic-node-completion",
  "status": "success",
  "summary": "完成 CLI 参数解析模块分析",
  "next": {
    "type": "single",
    "node": {}
  }
}
```

### 10.1 next=end

当前链路结束：

```json
{
  "version": "0.1",
  "kind": "dynamic-node-completion",
  "status": "success",
  "summary": "当前分支已完成",
  "next": {
    "type": "end"
  }
}
```

### 10.2 next=single

创建一个后继节点：

```json
{
  "version": "0.1",
  "kind": "dynamic-node-completion",
  "status": "success",
  "summary": "完成模块边界分析",
  "next": {
    "type": "single",
    "node": {
      "id": "node-dev-cli",
      "kind": "workflow-invocation",
      "title": "实现 CLI 模块",
      "workflowId": "dev-review-test-accept",
      "task": "在独立 worktree 中重构 CLI 参数解析模块",
      "workspace": {
        "mode": "worktree"
      },
      "dependsOn": ["node-plan-cli"]
    }
  }
}
```

### 10.3 next=fanout

创建多个并行节点：

```json
{
  "version": "0.1",
  "kind": "dynamic-node-completion",
  "status": "success",
  "summary": "完成整体拆分",
  "next": {
    "type": "fanout",
    "groupId": "group-core-refactor",
    "nodes": [
      {
        "id": "node-dev-cli",
        "kind": "workflow-invocation",
        "title": "重构 CLI 模块",
        "workflowId": "dev-review-test-accept",
        "task": "在独立 worktree 中完成 CLI 参数解析模块重构",
        "workspace": {
          "mode": "worktree"
        },
        "dependsOn": ["node-plan"]
      },
      {
        "id": "node-dev-config",
        "kind": "workflow-invocation",
        "title": "重构配置模块",
        "workflowId": "dev-review-test-accept",
        "task": "在独立 worktree 中完成配置加载模块重构",
        "workspace": {
          "mode": "worktree"
        },
        "dependsOn": ["node-plan"]
      }
    ],
    "merge": {
      "title": "合并核心模块重构结果",
      "provider": "claude-acp",
      "profile": "dev",
      "task": "合并 group-core-refactor 下所有 worktree 修改，解决冲突并输出合并总结"
    },
    "acceptance": {
      "title": "验收核心模块重构结果",
      "provider": "claude-acp",
      "profile": "accept",
      "task": "验证 group-core-refactor 合并后的结果是否满足需求"
    }
  }
}
```

### 10.4 status

V1 先支持：

```ts
type DynamicCompletionStatus = 'success';
```

失败路径后续扩展。

如果内部节点失败，V1 可直接让 AI-DYNAMIC pause 或 failure。

---

## 11. Fanout / Group / Join

AI-DYNAMIC 内部任何 fanout 都必须创建 group。

```text
node-1
  -> fanout group-A
      ├─ node-2
      └─ node-3
```

runtime 创建 group state：

```json
{
  "id": "group-A",
  "status": "open",
  "rootNodeIds": ["node-2", "node-3"],
  "endedNodeIds": [],
  "mergeNodeId": null,
  "acceptanceNodeId": null
}
```

### 11.1 group 生命周期

```ts
type DynamicGroupStatus =
  | 'open'
  | 'merge-ready'
  | 'merging'
  | 'merged'
  | 'accepting'
  | 'accepted'
  | 'closed'
  | 'failed';
```

流程：

```text
open
  -> 所有 group 分支 next=end
  -> merge-ready
  -> 创建 merge agent node
  -> merging
  -> merge node success
  -> merged
  -> 创建 acceptance agent node
  -> accepting
  -> acceptance node success
  -> accepted
  -> closed
```

### 11.2 group 结束条件

group 不是 agent 自己说结束。

runtime 派生：

- group 下所有已 materialized chain 都到 `end`
- 没有 ready/running/pending 节点
- 没有未 materialize 的 accepted proposal
- 没有 failed 节点
- 没有 unresolved validation error

满足后进入 `merge-ready`。

### 11.3 group 内 fanout

技术上可以，但 V1 建议限制：

```json
"maxGroupDepth": 1
```

也就是：

- AI-DYNAMIC 内部可以有多个 fanout group。
- V1 不允许 group 内再创建嵌套 group。
- 后续再扩展 group nesting。

---

## 12. AI-DYNAMIC 完成条件

AI-DYNAMIC 的完成不是 agent 决定，而是 runtime 派生。

满足以下条件，AI-DYNAMIC 才能 completed/success：

1. 所有内部 chain 已 terminal。
2. 所有 group 已 closed。
3. 所有 merge node success。
4. 所有 acceptance node success。
5. 没有 ready/running/pending internal node。
6. 没有未处理 dynamic-node-completion proposal。
7. 没有 validation error。
8. 没有 failed node。

然后：

```text
AI-DYNAMIC node outcome = success
外层 workflow 继续走 success edge
```

---

## 13. 状态模型

建议新增 AI-DYNAMIC 内部状态，不要把内部节点塞进外层 round trace。

### 13.1 DynamicRunState

```ts
interface DynamicRunState {
  version: string;
  id: string;
  parentRunId: string;
  parentRoundId: string;
  parentNodeId: string;
  parentAttemptId: string;
  status: 'running' | 'paused' | 'completed';
  outcome?: 'success' | 'failure' | 'killed' | null;
  startedAt: string;
  updatedAt: string;
  control: DynamicControl;
  allowedWorkflowSnapshots: AllowedWorkflowSnapshot[];
  currentNodeIds: string[];
}
```

### 13.2 DynamicNodeState

```ts
interface DynamicNodeState {
  version: string;
  id: string;
  dynamicRunId: string;
  kind: 'worker' | 'workflow-invocation' | 'merge' | 'acceptance';
  title: string;
  task: string;
  status: 'pending' | 'ready' | 'running' | 'paused' | 'completed';
  outcome?: 'success' | 'failure' | 'killed' | null;
  groupId?: string | null;
  chainId: string;
  depth: number;
  dependsOn: string[];
  workspace: WorkspacePolicy;
  workspacePath?: string | null;
  provider?: string | null;
  profile?: string | null;
  workflowId?: string | null;
  workflowSnapshotId?: string | null;
  childRunId?: string | null;
  startedAt?: string | null;
  finishedAt?: string | null;
}
```

### 13.3 DynamicGroupState

```ts
interface DynamicGroupState {
  version: string;
  id: string;
  dynamicRunId: string;
  status: DynamicGroupStatus;
  depth: number;
  parentGroupId?: string | null;
  rootNodeIds: string[];
  terminalNodeIds: string[];
  mergeNodeId?: string | null;
  acceptanceNodeId?: string | null;
  createdByNodeId: string;
  createdAt: string;
  updatedAt: string;
}
```

### 13.4 DynamicProposalState

```ts
interface DynamicProposalState {
  version: string;
  id: string;
  dynamicRunId: string;
  sourceNodeId: string;
  artifactPath: string;
  rawOutputPath: string;
  parsed: unknown;
  validationStatus: 'pending' | 'accepted' | 'rejected';
  validationErrors: string[];
  materializedEventIds: string[];
  createdAt: string;
}
```

---

## 14. 文件结构

AI-DYNAMIC 内部状态放在该节点 attempt 目录下，避免污染外层 run/round：

```text
runs/run-001/
  rounds/round-001/
    nodes/ai-dynamic/
      attempt-001/
        node.json
        dynamic/
          dynamic-run.json
          allowed-workflow-snapshots.json
          graph.json
          events.jsonl
          groups/
            group-core-refactor.json
          nodes/
            node-plan/
              node.json
              attempt-001/
                artifacts/
                attachments/
                raw.stream.jsonl
                acp.events.jsonl
            node-dev-cli/
              node.json
              attempt-001/
                artifacts/
                attachments/
            node-dev-config/
              node.json
              attempt-001/
                artifacts/
                attachments/
            group-core-refactor-merge/
              node.json
              attempt-001/
                artifacts/
                attachments/
            group-core-refactor-accept/
              node.json
              attempt-001/
                artifacts/
                attachments/
          proposals/
            proposal-node-plan-001.json
```

外层 round graph 只显示 `AI-DYNAMIC` 一个节点。

进入该节点后，展示内部 dynamic graph。

---

## 15. workspace / worktree 策略

内部节点 workspace 策略：

```ts
type WorkspaceMode = 'readonly' | 'worktree' | 'main';
```

### 15.1 readonly

用于：

- 分析
- 审查
- 方案
- 只读验证

### 15.2 worktree

用于：

- 并行开发
- 可能修改代码的分支任务

每个 mutating fanout 分支默认一个独立 worktree。

### 15.3 main

用于：

- merge agent
- final acceptance
- cleanup

### 15.4 runtime 职责

runtime 负责：

- 创建 worktree
- 记录 workspace path
- 把 workspace path 注入 prompt
- 在 merge 节点输入里列出相关 worktree
- 成功后按策略清理 worktree

runtime 不负责：

- 写业务代码
- 合并代码
- 解决冲突

这些由 agent 节点处理。

---

## 16. Prompt 设计

### 16.1 AI-DYNAMIC bootstrap prompt

AI-DYNAMIC 初始节点拿到：

- 原始 requirement
- 当前 task/run/round/node/attempt
- allowed workflow snapshots 摘要
- 可用 profile 列表
- 可用 provider 列表
- workspace policy
- dynamic control limits
- 输出 schema：`dynamic-node-completion`

它的任务是创建第一批内部节点。

### 16.2 内部 dynamic node prompt

每个内部节点拿到的 prompt 分两层：

system prompt：

- 当前 dynamic node id / group id / chain id / depth
- dependsOn 节点摘要
- upstream artifact/attachment 路径
- workspace path / workspace mode
- allowed workflow snapshots 摘要
- 当前 dynamic graph 摘要
- 可用 provider 列表
- 可用 profile 列表
- 当前剩余预算摘要（例如 remaining dynamic nodes、remaining workflow invocations、当前 fanout 能力）
- 输出 schema：`dynamic-node-completion`
- 不主动扫描 dynamic run 目录、只读取 prompt 明确给出的路径等 runtime 规则

user prompt：

- 原始 requirement
- 当前 sub-task / 当前 goal

要求：

- 不主动扫描 dynamic run 目录。
- 只读取 prompt 明确给出的路径。
- 完成后必须输出 `dynamic-node-completion`。
- 如果没有后续任务，输出 `next.type=end`。
- 如果需要并行，输出 `next.type=fanout`。

### 16.3 workflow-invocation prompt

如果内部节点是 workflow invocation：

- 它本身不直接跑 agent，而是由 runtime 启动 child workflow run。
- child workflow 的 requirement 继承原始 requirement。
- child workflow 的 user prompt task 由 invocation node 的 `task` 与 child node 原始 `goal` 通过 `src/prompts/<lang>/runtime/ai-dynamic/workflow_invocation.md` 包装得到。
- child workflow 完成后，该 invocation node success/failure。

V1 推荐 runtime 根据 child run outcome 包装 completion：

```json
{
  "version": "0.1",
  "kind": "dynamic-node-completion",
  "status": "success",
  "summary": "workflow dev-review-test-accept completed successfully",
  "next": {
    "type": "end"
  },
  "source": {
    "kind": "workflow-run",
    "childRunId": "run-002"
  }
}
```

如果后续需要 child workflow 完成后继续拆分，可增加 post-workflow router 节点，不让普通固定 workflow 混入 dynamic 控制语义。

---

## 17. runtime 调度流程

### 17.1 外层进入 AI-DYNAMIC

```text
outer orchestrator enters AI-DYNAMIC node
  -> create dynamic-run
  -> freeze allowed workflow snapshots
  -> create bootstrap internal node
  -> drive dynamic graph
```

### 17.2 内部 node 执行完成

```text
internal node completed
  -> read dynamic-node-completion artifact
  -> validate schema
  -> validate budget / dependency / group / workflow reference
  -> save proposal
  -> materialize next
```

### 17.3 materialize next=end

```text
mark current chain terminal
check group readiness
check dynamic completion
```

### 17.4 materialize next=single

```text
create one internal node
check dependsOn
if dependencies satisfied -> ready
```

### 17.5 materialize next=fanout

```text
create group
create child nodes
assign groupId / chainId
enqueue ready nodes
```

### 17.6 group merge

```text
all group chains terminal
  -> group merge-ready
  -> runtime creates merge node
  -> execute merge agent
  -> merge success
  -> runtime creates acceptance node
  -> execute acceptance agent
  -> acceptance success
  -> group closed
```

### 17.7 AI-DYNAMIC success

```text
no pending/running nodes
all groups closed
all chains terminal
  -> AI-DYNAMIC node success
  -> outer workflow continues
```

---

## 18. 校验规则

runtime 必须校验每个 proposal。

### 18.1 节点校验

- id 非空
- id 不重复
- kind 合法
- title/task 非空
- provider 可用；internal worker、merge、acceptance 的角色与任务约束由 `src/prompts` 内置 prompt 提供
- workspace mode 合法
- dependsOn 指向已存在节点
- 不形成环
- depth 不超过 maxDepth
- 总节点数不超过 maxDynamicNodes

### 18.2 fanout 校验

- groupId 非空
- groupId 不重复
- nodes 数量不超过 maxFanout
- group depth 不超过 maxGroupDepth
- 每个 fanout child 必须绑定 groupId
- merge 配置存在
- acceptance 配置存在

### 18.3 workflow invocation 校验

- workflowId 在 allowed workflow snapshots 中
- snapshot 合法
- 如果 `allowNestedDynamic=false`，snapshot 不包含 AI-DYNAMIC
- workspace mode 与 workflow invocation policy 兼容
- child workflow 启动时使用 snapshot，不使用 live workflow

### 18.4 幂等校验

每个 internal node 只能有一个 accepted completion proposal。

如果 crash 后重放：

- 相同 proposal id 不重复 materialize
- child node id 由 sourceNodeId + proposal item id 派生
- 已 materialized 的 node 不重复创建
- event log 可 replay

---

## 19. 预算限制

V1 必须内置限制：

```json
{
  "maxDynamicNodes": 20,
  "maxFanout": 5,
  "maxDepth": 6,
  "maxGroupDepth": 1,
  "maxParallel": 3,
  "maxWorkflowInvocations": 10,
  "allowNestedDynamic": false
}
```

这些限制属于 runtime validation，不只是 prompt 提示。

---

## 20. 失败处理 V1

V1 先做成功路径，但失败不能隐式。

建议简单规则：

```text
任一 internal node failure
  -> AI-DYNAMIC paused/error-blocked 或 failure

任一 proposal invalid
  -> runtime 先给当前 internal worker 一个隐藏 repair prompt，要求它基于校验错误自修复；最多重试 3 次，耗尽后才进入 AI-DYNAMIC paused/error-blocked

merge failure
  -> AI-DYNAMIC paused/error-blocked

acceptance failure
  -> AI-DYNAMIC failure 或外层 failure edge
```

暂时不做：

- route-decision
- replan
- partial retry
- group 局部失败恢复
- failed branch 替代方案

---

## 21. UI 方案

### 21.1 Workflow 编辑页

在工作流编辑器中新增能力开关：

```text
允许 AI 动态节点
```

打开后，节点新增菜单里出现：

```text
AI-DYNAMIC
```

AI-DYNAMIC 节点配置区默认先显示节点 ID，然后提供四个默认收起的编辑块：

- 基础信息：allowed workflows、maxDynamicNodes、maxFanout、maxDepth、maxParallel、maxGroupDepth、maxWorkflowInvocations、allowNestedDynamic
- Fan-out Agent：provider
- Merge Agent：provider
- Acceptance Agent：provider

allowed workflows 使用可搜索多选下拉栏，按可选/不可选分组展示 workflow。不可选项禁用并显示原因；默认工作流不豁免 `workflow.id` 重复限制。触发器内展示已选 workflow 标签，标签展示 workflow 名称与 DSL `workflow.id`，并可直接删除。`allowedWorkflows.workflowId` 存储 workflow 定义内的 `id`，不使用模板外层 `template.id`。

每个 workflow 显示：

```text
开发-审查-测试-验收
workflow.id = dev-review-test-accept
合法
不含 AI-DYNAMIC
```

如果包含 AI-DYNAMIC 且不允许嵌套：

```text
不可用：包含 AI-DYNAMIC
```

### 21.2 外层 Round 图

AI-DYNAMIC 作为一个节点显示：

```text
AI-DYNAMIC
running
internal nodes: 8
groups: 2
current: merge group-core-refactor
```

点击：

- 选中 AI-DYNAMIC
- 下方信息流显示 dynamic summary

双击：

- 打开内部 dynamic graph

### 21.3 内部 Dynamic Graph

展示：

- internal worker nodes
- workflow-invocation nodes
- group boundary
- merge nodes
- acceptance nodes

示例：

```text
bootstrap
  -> plan
    -> group-core-refactor
       ├─ impl-cli-flow
       └─ impl-config-flow
    -> merge-core-refactor
    -> accept-core-refactor
  -> end
```

workflow-invocation 节点显示 child run 状态：

```text
impl-cli-flow
workflow: dev-review-test-accept
child run: success
workspace: worktree
```

group 边界显示：

```text
group-core-refactor
2 / 2 ended
merge: running
acceptance: pending
```

### 21.4 详情抽屉

AI-DYNAMIC 详情抽屉展示：

- Summary
- Internal Graph
- Groups
- Proposals
- Artifacts
- Sessions
- Raw events

Dynamic node 详情展示：

- node summary
- task
- provider/profile
- workspace path
- dependsOn
- group/chain
- artifacts
- attachments
- ACP session
- completion proposal

workflow-invocation 详情展示：

- workflow snapshot
- child run
- child round list
- child graph deep link

---

## 22. 对现有代码的影响

### 22.1 DSL

当前 `NodeDsl` 只有 worker，需要新增 AI-DYNAMIC：

```rust
pub enum NodeDsl {
    Worker(WorkerNode),
    AiDynamic(AiDynamicNode),
}
```

影响：

- `src/dsl/mod.rs`
- validate_workflow
- workflow editor TS types
- graph rendering

### 22.2 runtime state

新增 dynamic state：

- `DynamicRunState`
- `DynamicNodeState`
- `DynamicGroupState`
- `DynamicProposalState`
- `AllowedWorkflowSnapshot`

建议新增模块：

```text
src/dynamic/
```

或：

```text
src/app/dynamic_orchestrator.rs
src/runtime/dynamic.rs
```

### 22.3 orchestrator

现有 `execute_ai_node()` 只处理普通 worker。

需要在 outer orchestrator 中识别：

```text
NodeDsl::AiDynamic
  -> execute_ai_dynamic_node()
```

AI-DYNAMIC 内部由独立 dynamic orchestrator 驱动。

### 22.4 provider prompt bundle

需要支持 dynamic context：

- dynamicRunId
- internalNodeId
- groupId
- chainId
- allowedWorkflowSnapshots
- upstream artifacts
- workspace path
- dynamic completion schema

### 22.5 view model

`GraphVm` 需要支持：

- node kind
- compound node
- child graph link
- group boundary
- workflow invocation child run

---

## 23. V1 开发拆分建议

### Phase 1：DSL + UI 配置

- 新增 AI-DYNAMIC 节点类型
- 工作流编辑器增加“允许 AI 动态节点”开关
- 开关打开后允许添加 AI-DYNAMIC 节点
- 配置 allowed workflows
- 保存时校验 allowed workflows 是否含 AI-DYNAMIC
- run start 冻结 allowed workflow snapshots

### Phase 2：Dynamic state + bootstrap

- 创建 dynamic-run state
- 创建 bootstrap internal node
- bootstrap 输出 dynamic-node-completion
- runtime 校验 artifact
- 支持 next=end / next=single

### Phase 3：内部 worker node

- 支持内部 worker 执行
- 支持 dynamic prompt context
- 支持 completion proposal
- 支持链式 single 节点

### Phase 4：fanout group

- 支持 next=fanout
- group state
- parallel ready queue
- maxParallel
- group all-ended 检测

### Phase 5：merge + acceptance agent

- group merge-ready 后创建 merge node
- merge success 后创建 acceptance node
- acceptance success 后 group closed
- 所有 group closed 后 AI-DYNAMIC success

### Phase 6：workflow-invocation

- 内部节点引用 allowed workflow snapshot
- 启动 child run
- child run success => dynamic node success
- UI deep link 到 child run/round

### Phase 7：UI internal graph

- 外层 AI-DYNAMIC 节点聚合状态
- 双击进入 internal graph
- group boundary
- proposal/artifact/session drawer

### Phase 8：失败与恢复增强

- invalid proposal pause
- failed node pause
- retry
- replan
- group 局部恢复
- nested dynamic 可选开启

---

## 24. 最终定义

`AI-DYNAMIC` 是普通工作流中的一个特殊复合节点。

它内部运行一个由 artifact 驱动的动态图。内部 agent 每次完成后输出统一 schema 的 completion proposal；runtime 校验 proposal 后创建后继 worker、workflow invocation、fanout group 或 end。

fanout group 的所有链路结束后，runtime 创建 merge agent，再创建 acceptance agent。

全部 group closed 且无 pending/running 内部节点后，AI-DYNAMIC 节点 success，外层普通工作流继续。

V1 边界：

- 不做 direct mode。
- 不做 triage-result。
- 不做 route-decision/replan。
- 不做 nested AI-DYNAMIC。
- 支持内部 fanout。
- 支持 workflow-invocation，但只引用 run start 时冻结的 allowed workflow snapshots。
- merge / acceptance 都是 agent 节点。
- AI-DYNAMIC 完成由 runtime 派生，不由 agent 自述决定。
