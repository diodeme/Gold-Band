# Gold Band 目录布局

## 1. 一句话定义

Layout 定义 Gold Band 的文件边界：项目仓库只保留项目级可覆盖配置；所有 task、authoring、run、round、attempt、ACP 会话、日志、artifact 和 attachment 等过程状态都存放在用户目录的 per-project runtime store。

---

## 2. 设计目标

目录结构服务于：

- 不污染真实项目工作树
- 可追溯
- 可恢复
- 可审计
- 节点产物稳定引用
- workflow / profile 的分层解析
- provider-specific 与 provider-agnostic 内容边界清晰

---

## 3. 顶层原则

### 3.1 项目目录只放配置覆盖

`<repo>/.gold-band` 不再承载 task、authoring、runs、logs 或 ACP runtime 文件，只作为项目级 Gold Band 配置覆盖目录。

### 3.2 过程状态全部属于 user project runtime

具体任务和执行过程都属于 Gold Band runtime 数据，统一放在：

```text
~/.gold-band/projects/{project-id}/
```

其中 `{project-id}` 直接由仓库绝对路径转义得到，采用 Claude Code 类似的可读目录名（例如 `D--Projects-code-ai-Gold-Band`），用于把不同项目的任务和运行状态隔离开。

### 3.3 顶层对象不是 conversation，而是 task

- 新需求 = 新 task
- 同一需求的再次执行 = 同 task 下新 run
- 同一 run 中的验收新一轮 = 新 round
- 同一 round 中某个节点的一次执行 = 一个 attempt
- task 级 authoring workflow 可编辑；run 创建时冻结为 `workflow.snapshot.json`，round 详情只读取运行时快照

### 3.4 session 可以复用，但 attempt 目录绝不能复用

- session 可以复用既有会话上下文继续执行
- 但每次节点执行都必须新建 `attempt-*`
- 任何一次执行都不能覆盖上一次产物

### 3.5 runtime 只信规范化产物，不信模型自己起的文件名

- canonical artifacts 必须由 runtime 规范化落盘
- 模型自由创建的文件只能作为 side effects 或 attachments
- 后续节点不应直接依赖模型自己起名的路径

---

## 4. 用户目录结构

用户目录用于存放跨项目配置和 per-project 运行数据。

建议位置：

```text
~/.gold-band/
```

推荐结构：

```text
~/.gold-band/
  config.json
  context/
    profiles/
      开发-profile-dev.md
  presets/
    workflows/
  providers/
    claude-acp/
  projects/
    D--Projects-code-ai-Gold-Band/
      project.json
      context/
        profiles/
          项目开发-profile-1760000000000000000.md
      logs/
        runtime.log
      tasks/
        task-001/
```

### 4.1 用户级 context

`context/profiles/` 存放跨项目复用的用户级 profile。profile 以 Markdown 文件存储，文件名为 `<name>-<id>.md`，正文顶部包含 `---` 信息块，声明 `id`、`name`、`summary`、`createdAt`、`updatedAt`；`id` 由系统生成分布式唯一值，时间字段使用本地时区 `YYYY-MM-DD HH:MM:SS`。

### 4.2 projects/{project-id}

`projects/{project-id}` 存放某个仓库对应的全部 Gold Band 过程状态，包括 task authoring、run 状态、ACP runtime 文件、artifacts、attachments 和 logs。

`project.json` 记录该 runtime store 对应的仓库路径和 project id，例如：

```json
{
  "version": "0.1",
  "projectId": "D--Projects-code-ai-Gold-Band",
  "repoRoot": "D:/Projects/code/ai/Gold-Band",
  "normalizedRepoRoot": "d:/projects/code/ai/gold-band"
}
```

---

## 5. 项目目录结构

项目目录只用于可覆盖配置，不存放过程状态。

推荐结构：

```text
<repo>/.gold-band/
  presets/
    workflows/
  config.json        # 如后续需要项目级配置覆盖
```

项目级 profile 不写入真实项目工作树，而是写入对应 user project runtime store：

```text
~/.gold-band/projects/{project-id}/context/profiles/<name>-<id>.md
```

### 边界

以下内容不应写入 `<repo>/.gold-band`：

- `tasks/**`
- `logs/**`
- `runs/**`
- `authoring/**`
- `artifacts/**`
- `attachments/**`
- ACP runtime 文件
- provider pid / permission / cancel marker 等控制文件

后续可以新增“最终态文档导出”能力，把过程中的 artifacts / attachments 汇总成用户明确需要的项目文档；但那是独立导出步骤，不是 runtime 默认落盘位置。

---

## 6. workflow / provider / profile 解析优先级

### 6.1 workflow 解析优先级

建议统一为：

1. CLI / UI 显式覆盖参数
2. 当前 task 的 user runtime authoring workflow
3. 项目目录下的预设 workflow
4. 用户目录下的预设 workflow

### 6.2 provider 解析优先级

建议统一为：

1. 当前节点显式声明的 `provider`
2. runtime 内部默认 provider（当前为 `claude-acp`）

### 6.3 profile 解析优先级

建议统一为：

1. `~/.gold-band/projects/{project-id}/context/profiles/<name>-<id>.md` 中的项目级 profile
2. `~/.gold-band/context/profiles/<name>-<id>.md` 中的用户级 profile

workflow DSL 中的 `profile` 字段保存 profile `id`，不保存文件名或显示名称；运行时通过扫描上述两个目录解析，项目级优先。开发阶段不保留旧固定 profile id 的迁移兼容分支，历史固定 ID 若仍存在会按普通 profile 处理。

---

## 7. task 目录结构

每个 task 目录对应一个用户任务，位于 user project runtime store。

```text
~/.gold-band/projects/{project-id}/
  tasks/
    task-001/
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

### 文件职责

- `task.json`：任务级元数据
- `authoring/requirement.md`：需求文本
- `authoring/workflow.json`：任务使用的 workflow authoring 输入
- `authoring/workflow.resolved.json`：本任务解析后的 workflow
- `authoring/provenance.json`：解析来源与 provenance
- `runs/`：该 task 下的每一次执行

---

## 8. run 目录结构

每个 run 目录对应这个 task 的一次完整执行。

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

### 文件职责

- `run.json`：本次执行的全局状态
- `run-progress.json`：本次 run 的快速状态视图
- `workflow.snapshot.json`：本次 run 真正执行的 workflow 快照
- `events.jsonl`：run 级时间线
- `rounds/`：本次 run 内的所有 round

---

## 9. round 目录结构

每个 round 对应一次 acceptance round。

```text
rounds/
  round-001/
    round.json
    nodes/
  round-002/
    round.json
    nodes/
```

---

## 10. attempt 目录结构

每个节点每执行一次，就新建一个 attempt 目录。

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

即使 session 复用，同一个节点每次执行也必须新建 attempt。

---

## 11. AI worker attempt 结构

```text
attempt-001/
  node.json
  worker-ref.json
  provider.pid
  acp.snapshot.json
  acp.timeline.jsonl
  acp.raw.jsonl
  acp.diagnostics.jsonl
  # 旧会话目录可额外存在：
  # acp.session.json
  # acp.events.jsonl
  acp.diagnostics.jsonl
  acp.permission-request.<id>.json
  acp.permission-response.<id>.json
  acp.cancel-requested
  progress.events.jsonl       # legacy 观测文件，仅历史兼容
  raw.stream.jsonl             # legacy 观测文件，仅历史兼容
  artifacts/
    节点输出产物.json
    验收输出产物.json
  attachments/
    report.md
```

### 文件职责

#### runtime 管理

- `node.json`
- `worker-ref.json`
- `provider.pid`
- `artifacts/`
- `attachments/`

#### ACP 运行态

- `acp.snapshot.json`：V2 UI runtime snapshot 与恢复锚点
- `acp.timeline.jsonl`：V2 聚合 UI timeline final item 列表
- `acp.raw.jsonl`：ACP 原始 frame
- `acp.diagnostics.jsonl`：adapter / protocol diagnostics
- `acp.session.json` / `acp.events.jsonl`：仅历史旧会话可能存在，供 legacy reader 兼容读取
- `acp.diagnostics.jsonl`：adapter / protocol diagnostics
- permission request / response 文件
- cancel marker

#### session identity

`worker-ref.json` 是 provider / ACP session identity 的唯一事实源。`continue_ref.acpSessionId` 决定后续 `session/load` 与 UI header 的 provider session id。

---

## 12. worker 节点 attempt 结构

```text
attempt-001/
  node.json
  artifacts/
    节点输出产物.json
  commands/
    01-build/
      command.json
      stdout.log
      stderr.log
    02-test/
      command.json
```

`worker` 节点的执行产物全部由 runtime 生成，并位于 user project runtime store。

---

## 13. artifacts 与 attachments 的边界

### 13.1 artifacts

只放 canonical artifact，例如：

- `节点输出产物.json`
- `节点输出产物.json`
- `验收输出产物.json`

特点：

- 文件名固定
- schema 固定
- 由 runtime 规范化落盘
- 可被下游节点程序化消费
- UI、CLI 和 selection 层优先使用不带 `.json` 的逻辑名

### 13.2 attachments

只放 free-form 附件，例如：

- `report.md`
- `analysis.md`
- `test-notes.md`

attachments 属于过程材料，默认仍放在 user project runtime store。后续若要进入项目工作树，应通过显式“最终态文档导出 / 汇总”步骤完成。

---

## 14. logs

runtime debug 日志位于：

```text
~/.gold-band/projects/{project-id}/logs/runtime.log
```

日志只用于 debug / 排障 / 运行分析，不属于 canonical state，不作为 UI 主数据源或控制流输入。

---

## 15. 总结

`<repo>/.gold-band` 是项目级配置覆盖目录；`~/.gold-band/projects/{project-id}` 是 Gold Band 对该项目的完整过程状态目录。所有过程文件默认不污染真实项目工作树。
