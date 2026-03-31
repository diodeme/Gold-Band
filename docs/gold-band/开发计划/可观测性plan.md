# Gold Band 日志 / 进度可观测性落地计划

## Context
当前 `cargo run -- run start task-001` 在真实执行时几乎没有过程反馈：CLI 只会在命令返回后打印最终 JSON，而 provider 调用期间用户看不到“现在正在做什么”；同时仓库里虽然已经有 progress 设计文档（`raw.stream.jsonl`、`progress.events.jsonl`、`run-progress.json`），但代码层还没有真正把这些观测文件打通。用户明确要求这一轮优先解决“过程黑盒”问题：控制台要有关键进度提示，但核心日志应以文件落盘为主，而且希望整个系统运行过程也能被记录下来；在 `progress.events.jsonl` 尚未完全设计实现前，可以先优先落地 `raw.stream.jsonl`。

另外，系统日志的定位已经进一步明确：它**只用于 debug / 排障 / 运行分析**，不属于 canonical state，也不作为 UI 主数据源或控制流输入；但它需要具备类似 Java logback 的可管理性，例如自动滚动、归档和保留策略。当前代码已经使用 `tracing` + `tracing-subscriber`（见 `src/main.rs` 和 `Cargo.toml`），因此推荐延续 tracing 生态，而不是切到另一套完全独立的日志体系。

## 推荐方案

### 1. 先落一版“系统日志 + run 进度 + raw stream”的最小闭环
这一轮不追求完整事件体系，而是先把最关键的三类观测数据打通：

- 系统级运行日志：记录 runtime / CLI / provider 关键行为
- run 级观测文件：`run-progress.json` + `events.jsonl`
- attempt 级 provider 原始流：`raw.stream.jsonl`

控制台只负责打印少量关键进度；完整日志以文件为准。

### 2. 用 tracing 生态落系统 debug 日志，并补 logback 风格自动滚动/归档
Rust 没有一个“官方 logback”，但 tracing 生态里有成熟的 rolling file appender 能力，最接近 logback 的做法是：
- 用 `tracing` 作为统一日志 API
- 用 `tracing-subscriber` 挂 subscriber / layer
- 再引入 rolling appender（如 tracing-appender 一类）实现按时间滚动

基于这个方向，这轮系统日志设计为：

- 在 repo 下新增系统日志目录：`<repo>/.gold-band/logs/`
- 输出全局 debug 日志文件，例如：
  - `runtime.log`（当前活动日志）
  - `runtime.yyyy-mm-dd.log` 或按小时/日期滚动出来的归档文件
- 系统日志只记录 runtime / CLI / provider / exec 的内部行为与异常
- run 内继续记录：
  - `run-progress.json`
  - `events.jsonl`
- attempt 内继续记录：
  - `raw.stream.jsonl`

为了贴近 logback 的体验，这轮建议至少实现：
- 自动滚动（优先按日，必要时可加按小时）
- 自动保留清理（例如只保留最近 N 天）
- 日志目录集中在 `.gold-band/logs/`

如果 tracing 现成 rolling appender 只能满足“按时间滚动”，而不能直接满足“按大小 + 历史清理”两者同时具备，那么这轮优先：
1. 先用 tracing 生态落稳定的 rolling file 输出
2. 再由 runtime 在启动时做一次归档文件清理（按文件名日期或 mtime）

这样实现复杂度可控，也足够接近 logback 的使用体验。

### 3. 在 `storage` 中统一补齐观测文件路径，并给日志上下文固定 execution key
扩展 `src/storage/mod.rs`，增加这些路径 helper：
- `logs_dir()`
- `runtime_log_file()`
- `run_progress_file(task_id, run_id)`
- `run_events_file(task_id, run_id)`
- `progress_events_file(task_id, run_id, round_id, node_id, attempt_id)`（即使这轮先不完全使用，也先把路径定出来）

同时统一约定每条 debug 日志和 run 级事件都尽量带上上下文字段：
- `traceId`：一次 CLI 调用链
- `taskId`
- `runId`
- `roundId`
- `nodeId`
- `attemptId`
- `executionKey`

其中 `executionKey` 推荐固定格式：
- `<taskId>/<runId>/<roundId>/<nodeId>/<attemptId>`

这个 key 的用途只是 grep / 排障 / 串联日志上下文，**不是业务主键，也不是 canonical contract**。

此外 storage 侧仍需要补最小 helper：
- append JSONL 事件
- best-effort 写入 progress snapshot
- 为日志/事件写入自动创建目录

原则：
- 这些观测写入失败不能影响主流程
- storage helper 统一负责建目录，避免各处散落 `create_dir_all`

### 4. 在 `orchestrator` 里落 run 级事件和进度快照
`src/app/orchestrator.rs` 是这轮最关键的挂点。

具体在这些点落盘：
- `run_start(...)`
  - 写系统日志：run 创建、workflow 加载、entry node 初始化
  - 写 `run-progress.json`：`starting`
  - append run `events.jsonl`：`run_started`
- `run_continue(...)`
  - 写系统日志：continue 尝试恢复什么 run / round / node / attempt
  - 写 `run-progress.json`
  - append run `events.jsonl`：`run_continue_requested`
- `drive_from_node(...)`
  - 节点开始前：`node_started`
  - 调 worker/verify 前：`calling_provider`
  - 调 exec 前：`running_command`
  - 节点完成后：`node_completed`
  - 节点切换时：`transitioned`
  - acceptance loop 新 round：`round_opened`
  - run pause：`run_paused`
  - run complete：`run_completed`

`run-progress.json` 的字段遵循 `docs/gold-band/interaction/progress.md` 的最小 schema，`currentStage` 这一轮至少使用：
- `starting`
- `calling_provider`
- `streaming`
- `normalizing_artifact`
- `running_command`
- `verifying`
- `paused`
- `blocked`
- `completed`

### 5. 在 `node_executor` 中开启 AI 节点 `raw.stream.jsonl`，并写系统日志
当前 `execute_ai_node(...)` 把 `stream_mode` 写死成 `StreamMode::None`。这轮改为：
- worker / verify 节点统一请求 `StreamMode::Raw`

同时在 `node_executor` 里补这些日志点：
- AI 节点 invocation 构造完成
- provider 返回 success / failure / interrupted
- artifact 正在规范化
- worker-ref 是否已落盘
- exec 节点开始执行 / 执行完成

这里不让日志改变控制流，只记录事实。

### 6. 在 Claude provider 中真正落 `raw.stream.jsonl`，并给出结果摘要日志
`src/provider/mod.rs` 当前虽然有：
- `StreamMode`
- `supports_raw_stream`
- `stream_path`

但实现还是 `command.output()`，没有真正写 raw stream。这轮在 provider 中做最小可行增强：

- 当 `req.stream_mode == StreamMode::Raw` 时：
  - 仍然调用 `claude`
  - 至少把 provider 的 stdout/stderr 原始内容落到 `raw.stream.jsonl`
  - `ProviderRunResult.stream_path` 返回该文件路径
- 如果无法做到真正边执行边增量写，也接受第一版先把最终 stdout/stderr 以 JSONL envelope 形式归档到 `raw.stream.jsonl`

这样虽然还不是完整“实时 streaming”，但至少：
- 原始 provider 输出被归档
- 用户可直接查看 raw.stream.jsonl
- 架构上已经把 raw stream 链路打通

这是当前架构下最稳妥的 MVP。

### 7. CLI 改成“stderr 打进度，stdout 保留最终 JSON”
`src/cli/mod.rs` 当前只在命令结束后打印最终 JSON。为了不破坏脚本兼容性：
- 最终 JSON 继续打印到 stdout
- 关键进度打印到 stderr

至少包括：
- 正在启动 run
- run id 已创建
- 正在执行哪个 round/node/attempt
- provider 调用开始 / 结束
- run 最终 paused/completed
- 提示用户可查看的文件路径（如 `run-progress.json` / `events.jsonl` / `raw.stream.jsonl`）

这样用户终端会有立即反馈，同时文件仍是主观察面。

### 8. 系统日志边界要严格收窄为 debug-only
实现时需要明确：
- `runtime.log` 及其归档文件是 debug 日志，不是业务产物
- schema 可以面向排障优化，但不要求像 canonical state 那样稳定
- UI/插件的主观测面仍然应该优先使用：
  - `run-progress.json`
  - `events.jsonl`
  - 后续 `progress.events.jsonl`
  - `raw.stream.jsonl`

也就是说：
- 状态真相看 `run.json / round.json / node.json / artifacts`
- 用户进度看 `run-progress.json / events / raw stream`
- 内部为什么会这样、调用边界和异常细节看 `runtime.log`

## 关键文件
重点修改：
- `src/storage/mod.rs`
- `src/app/orchestrator.rs`
- `src/app/node_executor.rs`
- `src/provider/mod.rs`
- `src/cli/mod.rs`

可选新增：
- `src/app/progress.rs` 或 `src/observability/mod.rs`
  - 用于封装 run-progress / events / runtime log 写入

## 复用现有实现
本轮应直接复用：
- `src/storage/mod.rs` 现有路径模型，尤其是 `raw_stream_file(...)`
- `docs/gold-band/interaction/progress.md` 里的最小 schema
- `docs/gold-band/runtime/layout.md` 中 run / attempt 级观测文件布局
- `src/app/orchestrator.rs` 现有生命周期编排点
- `src/provider/mod.rs` 现有 `StreamMode` / `stream_path` 契约

## 验证方案
### 自动化验证
- `cargo test --quiet`

### 人工验证
1. 执行：
   - `cargo run -- run start task-001`
2. 终端应立即看到 stderr 进度，而不是长时间无反馈
3. 检查系统日志：
   - `.gold-band/logs/runtime.log`
4. 检查 run 级文件：
   - `.gold-band/tasks/task-001/runs/run-001/run-progress.json`
   - `.gold-band/tasks/task-001/runs/run-001/events.jsonl`
5. 检查 attempt 级 raw stream：
   - `.gold-band/tasks/task-001/runs/run-001/rounds/round-001/nodes/dev/attempt-001/raw.stream.jsonl`
6. 若 run pause/fail：
   - 终端能看到关键状态
   - 文件里能追溯 provider failure / blocked 的上下文
7. `stdout` 仍保持最终 JSON 输出，不被进度文本污染

## 决策说明
这轮优先实现“文件日志为主、控制台关键进度为辅、raw stream 先打通”的方案，而不直接一步到位实现完整 `progress.events.jsonl`，原因是用户当前最痛的问题是执行过程完全黑盒，且 repo 已经有 `raw.stream.jsonl` 的规范和部分代码脚手架。先把系统日志、run 级快照/事件和 raw stream 归档打通，可以最小代价解决可观测性问题，同时不破坏现有 runtime / provider / control 边界；等这个基础稳定后，再继续把 `progress.events.jsonl` 做成真正 provider-agnostic 的规范化事件流。