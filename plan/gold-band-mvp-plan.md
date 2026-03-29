# Gold Band Rust MVP 实现方案

## 目标

先实现一条最小可用闭环：

1. 读取 task + workflow
2. 跑 `worker`
3. 若产出 `exec-plan`，跑 `exec`
4. 若有 `verify`，跑 `verify`
5. 按 control 规则做 `continue / retry / acceptance loop`
6. 通过 CLI 查看状态、artifact、open-session

原则：先跑通主链路，再补增强能力。

---

## MVP 功能边界

### 必做
- task / run 基础目录结构
- workflow snapshot
- DSL 解析与基本校验
- runtime state
  - `run.json`
  - `round.json`
  - `node.json`
  - `worker-ref.json`
- `worker` 调用 Claude Code
- `exec` 串行执行命令
- `verify` 调用 Claude Code
- canonical artifact 落盘
  - `exec-plan`
  - `exec-result`
  - `verify-result`
- control engine
- CLI
  - `run start`
  - `run status`
  - `run continue`
  - `run retry`
  - `run kill`
  - `artifact show/list`
  - `run open-session`

### 暂不做
- 多 provider 真正接入
- `progress.events` 精细事件模型
- raw stream 复杂映射
- VSCode 插件
- 复杂 doctor/test matrix
- 高级调度 / 多 run 并发 orchestration

---

## Rust 模块拆分

建议先用一个 binary crate，内部按模块拆，不急着一开始就上多 crate workspace。

```text
src/
  main.rs
  cli/
  app/
  domain/
  dsl/
  runtime/
  provider/
  exec/
  storage/
  control/
  artifacts/
  inspect/
  util/
```

---

## 模块职责

### 1. `cli/`
负责命令行入口和参数解析。

建议使用：
- `clap`

子命令先做：
- `task show`
- `run start <task-id>`
- `run status <run-id>`
- `run continue <run-id>`
- `run retry <run-id>`
- `run kill <run-id>`
- `run open-session ...`
- `artifact list/show`

CLI 只做参数解析和调用 app service，不直接碰底层细节。

### 2. `domain/`
放最核心的 typed model。

例如：
- `RunStatus = Running | Paused | Completed`
- `RunOutcome = Success | Failure | Killed`
- `NodeType = Worker | Exec | Verify`
- `NodeOutcome = Success | Failure | Invalid | Killed`
- `SessionMode = New | Continue`
- `ExecCommandStatus = Success | Failure | Skipped`
- `AcceptanceFailurePolicy = AutoLoop | Stop`

这一层尽量不依赖 IO，是整个项目的建模核心。

### 3. `dsl/`
负责 workflow DSL 的解析和校验。

包括：
- workflow 文件读入
- `nodes[] / edges[] / control`
- 合法性校验
- `$end`
- `goal -> taskInstruction` 的规则落地到 resolved config 前的准备

建议输出两层：
- `WorkflowDsl`：原始输入
- `ValidatedWorkflow`：校验后的可执行模型

### 4. `runtime/`
负责 run / round / node / attempt 的生命周期管理。

包括：
- 创建 run 目录
- 创建 round / attempt
- 写 `run.json`
- 写 `round.json`
- 写 `node.json`
- 写 workflow snapshot
- 更新 `currentRound/currentNode/currentAttempt`

### 5. `storage/`
负责文件系统读写和路径约定。

例如：
- `RunPaths`
- `AttemptPaths`
- artifact path resolver
- JSON read/write helpers
- atomic write

建议 runtime 不自己拼大量路径，统一走 storage/path builder。

### 6. `artifacts/`
负责 canonical artifact 的规范化、校验、落盘。

先做三类：
- `exec-plan`
- `exec-result`
- `verify-result`

职责：
- schema struct
- parse / validate
- write canonical json
- 从 provider result 提取并校验 primary artifact

### 7. `provider/`
负责 provider adapter 抽象和 Claude Code 实现。

建议先定义 trait：

```rust
trait ProviderAdapter {
    fn describe_provider(&self) -> ProviderInfo;
    fn doctor(&self) -> DoctorResult;
    fn run_worker(&self, req: WorkerInvocation) -> Result<ProviderRunResult>;
    fn open_session(&self, worker_ref: &WorkerRef) -> Result<()>;
}
```

内部再分：

#### `provider::invocation`
- A() 输入模型
- prompt bundle
- execution context

#### `provider::claude_code`
- Claude Code adapter
- prompt bundle -> Claude Code 命令映射
- session continue/new
- worker-ref seed 提取

MVP 只实现 `claude-code`。

### 8. `exec/`
负责执行 `exec-plan`。

包括：
- 读取当前 round 最新 `exec-plan`
- 串行执行 commands
- fail-fast
- 生成 `exec-result.json`
- 写 `stdout.log` / `stderr.log`

这一层不混 control 逻辑，只返回 exec 结果。

### 9. `control/`
MVP 核心。

负责：
- 根据 node result 归纳 outcome
- 查 edge
- 判断 `$end`
- 判断 `onAcceptanceFailure`
- 判断 repair loop / acceptance loop
- 计算下一步动作

建议做成纯逻辑模块：

输入：
- validated workflow
- current node
- node outcome
- runtime state
- capability info

输出：

```rust
enum ControlDecision {
    TransitionToNode { node_id: String, session: SessionMode },
    OpenNewRound,
    CompleteRunSuccess,
    CompleteRunFailure,
    PauseErrorBlocked,
    PauseInterrupted,
}
```

### 10. `app/`
应用服务层，串起 CLI、runtime、provider、exec、control。

例如：
- `start_run()`
- `continue_run()`
- `retry_run()`
- `kill_run()`
- `open_session()`

这层是 orchestration，不放太多 schema 细节。

---

## 核心执行主链路

### `run start`
MVP 主流程：

1. 读取 task
2. 解析 workflow
3. DSL 校验
4. 创建 run + `round-001`
5. 从 `entry` 开始执行 node

### 如果 node 是 `worker`
- resolve provider/profile
- 生成 invocation
- `goal -> taskInstruction`
- 调 provider
- 生成 artifact / worker-ref / node.json
- control 决定下一步

### 如果 node 是 `exec`
- 读取当前 round 最新 `exec-plan`
- 执行 commands
- 写 `exec-result`
- control 决定下一步

### 如果 node 是 `verify`
- 组装默认 evidence package
- 调 provider
- 写 `verify-result`
- control 决定下一步

循环直到：
- complete
- paused

---

## MVP 状态机建议

### `worker`
- `success`
- `failure`
- `invalid`
- `paused`

### `exec`
- `success`
- `failure`
- `invalid`

### `verify`
- `success`
- `failure`
- `invalid`

### continue / retry
- `continue`
  - resume current provider session
  - 或 re-evaluate current invalid attempt
- `retry`
  - always new attempt
  - manual retry default `session = new`

### 默认 repair 规则
- `exec.invalid`
  - 若无显式 edge，默认回 `planFrom`
  - 优先 `continue`
  - provider 不支持则降级 `new`

---

## MVP 文件落盘

### worker attempt
```text
attempt-001/
  node.json
  worker-ref.json
  artifacts/
    exec-plan.json   # 如果有
  attachments/
```

### exec attempt
```text
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
```

### verify attempt
```text
attempt-001/
  node.json
  worker-ref.json
  artifacts/
    verify-result.json
```

---

## 推荐 Rust 技术选型

### 必要库
- `clap`：CLI
- `serde` / `serde_json`：schema
- `anyhow`：应用层错误
- `thiserror`：领域错误
- `tokio`：异步进程 / IO
- `tracing`：日志
- `camino`：UTF-8 path
- `uuid` 或时间戳生成 run/attempt id
- `indexmap`：若需保留 DSL 顺序

### 可选
- `schemars`：后续做 JSON schema
- `toml` / `serde_yaml`：若以后支持其他配置格式

---

## MVP 实现顺序

### Phase 1：先把骨架跑通
1. domain enums / structs
2. DSL parser + validator
3. runtime/storage path layout
4. CLI `run start/status`

### Phase 2：接通 worker
5. provider trait
6. Claude Code provider MVP
7. worker invocation + prompt bundle
8. worker artifact normalize

### Phase 3：接通 exec / verify
9. exec runner
10. exec-result writer
11. verify invocation
12. verify-result writer

### Phase 4：控制流闭环
13. control engine
14. continue / retry / kill
15. acceptance loop
16. `$end`

### Phase 5：可用性命令
17. artifact list/show
18. open-session
19. inspect/status 细化

---

## MVP 验证标准

至少覆盖以下 4 条用例：

### 用例 1：`worker -> exec -> verify -> success`
- run 最终 `completed + success`

### 用例 2：`exec failure -> repair -> exec success -> verify success`
- repair loop 生效

### 用例 3：`verify failure -> auto_loop -> new round -> success`
- acceptance loop 生效

### 用例 4：`worker invalid / interrupted`
- `run continue` / `run retry` 行为符合文档

---

## 最小实现切片

### Slice 1
- DSL parser
- runtime layout
- `run start`
- 单 worker 节点
- worker artifact 落盘
- `run status`

### Slice 2
- `exec`
- `exec-result`
- repair loop

### Slice 3
- `verify`
- acceptance loop
- `$end`

### Slice 4
- `continue / retry / open-session`

---

## 结论

建议主实现语言使用 Rust，先围绕 CLI + runtime + Claude Code provider 跑通 MVP 闭环，再逐步补 provider 扩展、progress 观测与插件层。