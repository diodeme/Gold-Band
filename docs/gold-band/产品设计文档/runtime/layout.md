# Gold Band 目录布局

## 1. 一句话定义
Layout 用来定义 Gold Band runtime 的整体文件夹结构，包括：

- 用户目录下放什么
- 项目目录下放什么
- task / run / round / attempt 如何组织
- 哪些文件属于 canonical contract
- 哪些文件属于 free-form 附件或观测层

---

## 2. 设计目标
目录结构服务于：

- 可追溯
- 可恢复
- 可审计
- 节点产物稳定引用
- workflow / profile 的分层解析
- provider-specific 与 provider-agnostic 内容边界清晰

---

## 3. 顶层原则

### 3.1 顶层对象不是 conversation，而是 task
当前推荐模型：

- 新需求 = 新 task
- 同一需求的再次执行 = 同 task 下新 run
- 同一 run 中的验收新一轮 = 新 round
- 同一 round 中某个节点的一次执行 = 一个 attempt

### 3.2 session 可以复用，但 attempt 目录绝不能复用
- session 可以复用既有会话上下文继续执行
- 但每次节点执行都必须新建 `attempt-*`
- 任何一次执行都不能覆盖上一次产物

### 3.3 runtime 只信规范化产物，不信模型自己起的文件名
- canonical artifacts 必须由 runtime 规范化落盘
- 模型自由创建的文件只能作为 side effects 或 attachments
- 后续节点不应直接依赖模型自己起名的路径

---

## 4. 用户目录结构
用户目录用于存放**跨项目可复用的内容**。

建议位置：

```text
~/.gold-band/
```

### 推荐结构

```text
~/.gold-band/
  presets/
    workflows/
    profiles/
  providers/
    claude-code/
    codex/
    gemini-cli/
```

### 各目录职责

#### `presets/workflows/`
存放用户级 workflow 预设。

例如：
- 通用开发工作流
- 轻量修 bug 工作流
- PR review 工作流

#### `presets/profiles/`
存放用户级 profile 预设。

例如：
- `developer`
- `tester`
- `planner`
- `verifier`

#### `providers/<provider>/`
存放 provider 级用户配置或 provider-specific 扩展材料。

注意：
- 这里可以存 provider 的用户侧配置
- 但不应放 task/run 执行痕迹

---

## 5. 项目目录结构
项目目录用于存放**和当前仓库绑定**的工作流、profile、任务与运行数据。

建议位置：

```text
<repo>/.gold-band/
```

### 推荐结构

```text
<repo>/.gold-band/
  presets/
    workflows/
    profiles/
  logs/
    runtime.log
    runtime.log.YYYY-MM-DD
  tasks/
  index.json
```

### 各目录职责

#### `presets/workflows/`
存放项目级 workflow 预设。

#### `presets/profiles/`
存放项目级 profile 预设。

#### `tasks/`
存放当前项目的所有 task。

#### `logs/`
存放 runtime debug 日志。

当前实现：
- 主日志文件固定为 `runtime.log`
- provider command summary 与 prompt bundle summary 写入该日志
- 仅当 runtime 配置允许时，才记录完整 `system_prompt` / `user_prompt`

边界：
- 只用于 debug / 排障 / 运行分析
- 不属于 canonical state
- 不作为 UI 主数据源或控制流输入

#### `index.json`
用于快速索引当前项目下：
- 有哪些 task
- 每个 task 当前状态
- 最近的 run 是什么

---

## 6. workflow / provider / profile 解析优先级

### 6.1 workflow 解析优先级
建议统一为：
1. CLI 覆盖参数 `--workflow`
2. task 目录下的默认 workflow
3. 项目目录下的预设 workflow
4. 用户目录下的预设 workflow

### 6.2 provider 解析优先级
建议统一为：
1. 当前节点显式声明的 `provider`
2. runtime 内部默认 provider（当前 MVP 为 `claude-code`）

### 6.3 profile 解析优先级
建议统一为：
1. 项目目录下的 profile
2. 用户目录下的 profile

说明：
- 这些解析优先级应由 runtime 上层统一处理
- provider implementation 不应自行猜测 workflow / provider / profile 来源

---

## 7. task 目录结构
每个 task 目录对应一个用户任务。

### 推荐结构

```text
<repo>/.gold-band/
  tasks/
    task-20260320-001-login-error/
      task.json
      authoring/
        requirement.md
        workflow.json
        workflow.resolved.json
        provenance.json
      runs/
        run-001/
        run-002/
```

### 各部分职责

#### `task.json`
保存任务级元数据。

#### `authoring/`
保存需求与 workflow 的 authoring 输入。

建议包括：
- `requirement.md`
- `workflow.json`
- `workflow.resolved.json`
- `provenance.json`

#### `runs/`
保存该 task 下每一次执行。

---

## 8. run 目录结构
每个 run 目录对应这个 task 的一次完整执行。

### 推荐结构

```text
runs/
  run-001/
    run.json
    run-progress.json
    workflow.snapshot.json
    events.jsonl
    rounds/
      round-001/
      round-002/
```

### 各部分职责

#### `run.json`
保存本次执行的全局状态。

#### `run-progress.json`
保存本次 run 的快速状态视图，用于回答“当前 workflow 走到哪里了”。

#### `workflow.snapshot.json`
保存本次 run 真正执行的 workflow 快照。

#### `events.jsonl`
保存 run 级时间线。

#### `rounds/`
保存本次 run 内的所有大循环 round。

---

## 9. round 目录结构
每个 round 对应一次 acceptance round，也就是一次大循环。

### 推荐结构

```text
rounds/
  round-001/
    round.json
    nodes/
  round-002/
    round.json
    nodes/
```

### 语义
- `round-001`：初始执行
- `round-002`：第一次大循环
- `round-003`：第二次大循环

---

## 10. attempt 目录结构
每个节点每执行一次，就新建一个 attempt 目录。

### 推荐结构

```text
round-001/
  nodes/
    dev/
      attempt-001/
      attempt-002/
    run-tests/
      attempt-001/
    accept/
      attempt-001/
```

### 语义
- 小循环只新增 attempt，不新增 round
- 即使 session 复用，同一个节点每次执行也必须新建 attempt

---

## 11. `worker` 节点 attempt 结构

### 推荐结构

```text
dev/
  attempt-001/
    node.json
    worker-ref.json
    raw.stream.jsonl
    progress.events.jsonl
    artifacts/
      exec-plan.json
    attachments/
      report.md
```

### 文件职责

#### runtime 管理
- `node.json`
- `worker-ref.json`
- `progress.events.jsonl`
- `artifacts/` 目录结构
- `attachments/` 目录结构

补充约束：
- attempt 级观测不再单独维护 `progress.json`
- 当前 workflow 的快速状态视图统一由 run 级 `run-progress.json` 提供

#### provider / worker 执行产生
- `raw.stream.jsonl` 的内容源于 provider 流式输出
- `attachments/` 内的文件可由 worker 执行过程中自由创建
- 工作区中的源码/测试文件属于 workspace side effects，不属于 attempt 私有目录

---

## 12. `exec` 节点 attempt 结构

### 推荐结构

```text
run-tests/
  attempt-001/
    node.json
    exec-plan.source.json
    artifacts/
      exec-result.json
    commands/
      01-build/
        command.json
        stdout.log
        stderr.log
      02-test/
        command.json
        # 若该命令实际执行，则可有 stdout.log / stderr.log
        # 若该命令为 skipped，则不要求生成这些 sidecar
```

### 语义
- `exec` 节点的执行产物全部由 runtime 生成
- `exec` 节点 attempt 不再单独维护 `progress.json`
- 当前 workflow 的快速状态视图统一由 run 级 `run-progress.json` 提供
- 每条命令的执行状态、退出码与时间信息直接收敛进 [exec-result](../dsl/artifacts/exec-result.md) 的 `commands[]`
- 不存在自由 `attachments/` 的强需求，但允许后续扩展

---

## 13. `verify` 节点 attempt 结构

### 推荐结构

```text
accept/
  attempt-001/
    node.json
    worker-ref.json
    raw.stream.jsonl
    progress.events.jsonl
    artifacts/
      verify-result.json
    attachments/
      report.md
```

### 语义
- `verify` 也属于 AI worker 节点的一种执行形态
- 因此 layout 上与 `worker` 节点接近
- `verify` 节点 attempt 不再单独维护 `progress.json`
- 当前 workflow 的快速状态视图统一由 run 级 `run-progress.json` 提供
- 但它的 canonical artifact 是 `verify-result.json`

---

## 14. `artifacts/` 与 `attachments/` 的边界

### 14.1 `artifacts/`
只放 canonical artifact。

例如：
- `exec-plan.json`
- `exec-result.json`
- `verify-result.json`

特点：
- 文件名固定
- schema 固定
- 由 runtime 规范化落盘
- 可被下游节点程序化消费

### 14.2 `attachments/`
只放 free-form 附件。

例如：
- `report.md`
- `analysis.md`
- `test-notes.md`

特点：
- 文件名不参与 canonical contract
- 内容不参与控制流判断
- 只能通过 runtime 显式暴露的引用进入后续节点上下文

### 14.3 重要规则
- `artifacts/` 只放 canonical artifact
- `attachments/` 放自由格式附件
- `attachments/` 是上下文池，不是默认输入全集
- 后续节点不能默认扫描整个 `attachments/` 目录

---

## 15. 相关文档
- [Runtime 概览](overview.md)
- [控制层](control.md)
- [task.json](state/task.json.md)
- [run.json](state/run.json.md)
- [round.json](state/round.json.md)
- [node.json](state/node.json.md)
- [Worker Invocation Contract](../provider/invocation.md)
- [Worker Ref 规范](../provider/worker-ref.md)
- [Progress 规范](../interaction/progress.md)

---

## 16. 一句话总结

> **Gold Band 的 runtime layout 分成两层：用户目录存跨项目可复用的 workflow/profile/provider 配置，项目目录存 task/run/round/attempt 的执行数据；其中 `artifacts/` 保存 canonical contract，`attachments/` 保存 free-form 附件，workspace side effects 仍留在项目工作区。**
