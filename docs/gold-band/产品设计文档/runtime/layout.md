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

其中 `{project-id}` 直接由仓库绝对路径转义得到，采用 Claude Code 类似的可读目录名（例如 `D--Projects-code-ai-Gold-Band`），用于把不同项目的任务和运行状态隔离开。若当前 repo root 为文件系统根目录 `/` 等极端值，`project-id` 也必须稳定且非空，当前约定回落为 `root`，避免 runtime 文件直接散落在 `projects/` 根下。

### 3.3 顶层对象不是 conversation，而是 task

- 新需求 = 新 task
- 同一需求的再次执行 = 同 task 下新 run
- 同一 run 中的验收新一轮 = 新 round
- 同一 round 中某个节点的一次执行 = 一个 attempt
- task 级 authoring workflow 可编辑；run 创建时冻结为 `workflow.snapshot.json`，round 详情只读取运行时快照
- 新建 run（包括会话页重跑）必须基于该 task 下已存在 `run-*` 目录的最大数字后缀递增，并通过原子创建新 run 目录占位；不能根据当前选中的 run id 推导下一号，避免历史 run 已存在或并发重跑时覆盖/撞号。

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
  logs/
    runtime.log
  desktop/
    agent-diagnostics.json
  doctor/
    acp/                 # 临时 ACP 诊断目录；doctor 成功后删除，失败时只保留有界诊断 bundle
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
      tasks/
        task-001/
```

### 4.1 用户级 context

`context/profiles/` 存放跨项目复用的用户级 profile。profile 以 Markdown 文件存储，文件名为 `<name>-<id>.md`，正文顶部包含 `---` 信息块，声明 `id`、`name`、`summary`、`createdAt`、`updatedAt`；`id` 由系统生成分布式唯一值，时间字段使用本地时区 `YYYY-MM-DD HH:MM:SS`。

### 4.2 projects/{project-id}

`projects/{project-id}` 存放某个仓库对应的全部 Gold Band 过程状态，包括 task authoring、run 状态、ACP runtime 文件、artifacts、attachments。

系统级 debug 日志（`runtime.log`）为桌面/CLI 进程级全局日志，放在 `~/.gold-band/logs/`；桌面端启动时，即使当前还没有 task / run / ACP 事件，也必须先预创建 `~/.gold-band/logs/runtime.log`，保证首次启动、未选 workspace、目录选择器异常等问题都能有稳定的系统级排障落点。workspace 级过程日志仍通过 task/run/attempt 目录下的 `events.jsonl`、`run-progress.json`、`raw.stream.jsonl` 等文件保存。

凡是桌面端在主线程触发的原生文件/目录选择器，也必须使用非阻塞调用并通过回调或事件把结果回传到 runtime；不能在 workspace 选择、会话 workspace 添加等入口使用 blocking dialog API，否则会把“打开选择器”本身变成不可观测的卡死点。

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
- provider pid / permission request-response 等运行控制文件

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
  acp.elicitation-request.<id>.json
  acp.elicitation-response.<id>.json
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
- `acp.raw.jsonl`：ACP 原始 frame；超过内置 `acpRawMaxSizeBytes` 2MB 后滚动裁剪到约 `acpRawTargetSizeBytes` 1MB，并优先保留首个 `session/update` 前的初始化握手段
- `acp.diagnostics.jsonl`：adapter / protocol diagnostics
- `acp.session.json` / `acp.events.jsonl`：仅历史旧会话可能存在，供 legacy reader 兼容读取
- permission request / response 文件：文件名中的 `<id>` 必须使用 ACP JSON-RPC `session/request_permission` 的原始 request id。UI timeline 为了展示可使用 `permission-<id>` 这类稳定 item id，但响应文件、pending 文件和 VM `requestId` 都不能使用展示 id，否则 agent runtime 无法消费用户决策。
- elicitation request / response 文件：`acp.elicitation-request.<id>.json` 与 `acp.elicitation-response.<id>.json` 用于 ACP `elicitation/create` 的阻塞式表单交互。优先路径仍是后端 runtime 轮询响应文件解除等待，并在消费响应后持久化 `elicitationResponse` 与对应用户回答消息；但若会话已不再 active（例如应用关闭后重进、live waiter 已不存在），命令侧 `respond_elicitation` 必须补写同等 replay 事实到 `acp.timeline.jsonl`，保证 answered / skipped 状态可回放且不会在重进会话时重新弹出卡片。
- 应用关闭、启动恢复和显式 stop 不只收敛 workflow 当前 running run，也必须扫描并收敛所有仍标记为 active 的 ACP attempt。该规则同时覆盖 runtime 执行中的阻塞 elicitation、普通 follow-up ACP 会话以及 AI-DYNAMIC 内层 ACP attempt，统一把 pending permission / elicitation 写成可恢复的 cancelled / declined 事实，并更新 session snapshot。

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

系统级 runtime debug 日志位于：

```text
~/.gold-band/logs/runtime.log
```

桌面/CLI 进程启动时初始化全局 tracing subscriber，日志写入同一个全局文件；切换 workspace 不需要重新初始化日志 writer。如需按 workspace 过滤日志，应基于日志事件中的 `repo_root`、`project_id` 等字段过滤，而非按文件物理目录拆分。

workspace-scoped 执行过程日志仍按 task/run/attempt 维度保存：
  - `run-progress.json`
  - `events.jsonl`
  - `raw.stream.jsonl`（ACP attempt 目录下）

桌面端默认仅记录 INFO 级常规运行日志；用户可在「设置 → 高级 → 记录详细日志」中切换为 DEBUG 级，以便排障时即时放大日志粒度，无需重启客户端。

日志只用于 debug / 排障 / 运行分析，不属于 canonical state，不作为 UI 主数据源或控制流输入。

---

## 15. 总结

`<repo>/.gold-band` 是项目级配置覆盖目录；`~/.gold-band/projects/{project-id}` 是 Gold Band 对该项目的完整过程状态目录；`~/.gold-band/` 下 `logs/`、`desktop/`、`doctor/` 为用户/应用全局运行时数据。所有过程文件默认不污染真实项目工作树。
