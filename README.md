# Gold Band

Gold Band 是一个 Rust CLI/runtime，用来把一个任务按 workflow 编排成 `worker -> exec -> verify` 这样的多节点执行流程，并把全过程状态与产物持久化到仓库内的 `.gold-band/` 目录。

## Current MVP scope

当前代码里已经实现的能力：

- 读取 task authoring 输入并启动 run
- 执行 `worker` / `verify` 节点（默认 provider 为 `claude-code`）
- 执行 `exec` 节点并保存命令日志
- 持久化 run / round / attempt 状态
- 查看 task、run 状态和 artifacts
- 打开 provider 保存的会话命令
- 对暂停或失败中的 run 执行 `continue` / `retry` / `kill`

当前 CLI 以 [src/cli/mod.rs](src/cli/mod.rs) 为准，README 只覆盖当前已实现命令。

当前产品交互分为两种入口：
- scriptable subcommand CLI：面向脚本、自动化与插件调用
- command-driven console CLI：面向人工控制台操作、可视化 help 和详情下钻

约束：
- console CLI 只接受显式命令输入，不做自然语言解析
- runtime canonical truth 仍以 state files 与 canonical artifacts 为准
- provider 输出在 UI 中优先展示 `progress.events.jsonl`，不存在时回退到 `raw.stream.jsonl`

## Frontend rewrite status

原 Tauri + React 桌面前端已归档到 `.web_bak/`，当前主线保留 Rust CLI/runtime 与 console CLI，新的前端将重新设计后再接入。

## Modes

### 1. Scriptable subcommand CLI

```bash
gold-band task ...
gold-band run ...
gold-band artifact ...
```

### 2. Command-driven console CLI

```bash
gold-band console
```

在 console mode 中，用户通过显式命令进行操作，例如：

```text
/run --help
/run start task-001
/view artifacts
/view provider-output
```

## Prerequisites

- 较新的 Rust toolchain（项目使用 Rust 2024 edition）
- `cargo`
- `claude` CLI 在 PATH 中可用
- `claude` 已完成登录并可正常工作
- 从仓库根目录执行命令

建议先确认：

```bash
cargo test --quiet
claude --version
cargo run -- --help
```

注意：

- runtime 根目录固定在当前仓库的 `.gold-band/`
- `exec` 节点会在当前仓库工作区里执行命令，人工测试请在可接受的工作区中进行

## Runtime layout

Gold Band 会把运行数据写到：

```text
<repo>/.gold-band/
  tasks/
    <task-id>/
      task.json
      authoring/
        requirement.md
        workflow.json
      runs/
        <run-id>/
          run.json
          workflow.snapshot.json
          rounds/
            <round-id>/
              round.json
              nodes/
                <node-id>/
                  <attempt-id>/
                    node.json
                    worker-ref.json
                    artifacts/
                      exec-plan.json
                      exec-result.json
                      verify-result.json
```

更完整的布局说明见 [docs/gold-band/runtime/layout.md](docs/gold-band/runtime/layout.md)。

## Manual end-to-end walkthrough

下面这条 walkthrough 对齐当前测试里的 happy path：[tests/full_mvp_flow.rs](tests/full_mvp_flow.rs)。

### 1. 准备最小 task 输入

创建目录：

```bash
mkdir -p .gold-band/tasks/task-001/authoring
```

写入 `task.json`：

```json
{"version":"0.1","id":"task-001"}
```

保存到：

```text
.gold-band/tasks/task-001/task.json
```

写入 `requirement.md`：

```md
Implement feature
```

保存到：

```text
.gold-band/tasks/task-001/authoring/requirement.md
```

写入 `workflow.json`：

```json
{
  "version": "0.1",
  "id": "full-flow",
  "entry": "dev",
  "control": {
    "max_repair_loops": 1,
    "max_acceptance_loops": 1,
    "on_acceptance_failure": "auto-loop"
  },
  "nodes": [
    {
      "id": "dev",
      "type": "worker",
      "provider": "claude-code",
      "profile": "developer",
      "goal": "Create an exec plan",
      "primary_artifact": "exec-plan"
    },
    {
      "id": "run-tests",
      "type": "exec",
      "plan_from": "dev"
    },
    {
      "id": "accept",
      "type": "verify",
      "provider": "claude-code",
      "profile": "verifier"
    }
  ],
  "edges": [
    {"from": "dev", "to": "run-tests", "on": "success"},
    {"from": "run-tests", "to": "accept", "on": "success"}
  ]
}
```

保存到：

```text
.gold-band/tasks/task-001/authoring/workflow.json
```

### 2. 启动 run

```bash
cargo run -- run start task-001
```

预期输出是一段 JSON，其中通常会包含：

- `id: "run-001"`
- `status`
- `outcome`
- `current_round`
- `current_node`
- `current_attempt`

后续命令以下面假设的 `run-001` 为例。如果实际输出不是 `run-001`，请替换成真实值。

一个真实人工测试里，`run start` 也可能不会直接完成，而是暂停在当前节点。例如当 `claude` 返回失败、没有产出必需 artifact，或者需要人工介入时，run 可能进入：

- `status: "paused"`
- `pause_reason: "error-blocked"`

### 3. 查看 run 状态

```bash
cargo run -- run status task-001 run-001
```

如果 happy path 跑通，预期最终会看到类似：

- `status: "completed"`
- `outcome: "success"`

如果真实 provider 没有顺利产出结果，也可能看到：

- `status: "paused"`
- `pause_reason: "error-blocked"`

这不代表 runtime 布线有问题，更多时候表示真实 `worker` 调用这次没有成功完成。

### 4. 查看每个节点的 artifacts

查看 `dev` 节点产物：

```bash
cargo run -- artifact list task-001 run-001 --round round-001 --node dev --attempt attempt-001
```

查看 `run-tests` 节点产物：

```bash
cargo run -- artifact list task-001 run-001 --round round-001 --node run-tests --attempt attempt-001
```

查看 `accept` 节点产物：

```bash
cargo run -- artifact list task-001 run-001 --round round-001 --node accept --attempt attempt-001
```

直接查看 `verify-result` 内容：

```bash
cargo run -- artifact show task-001 run-001 --round round-001 --node accept --attempt attempt-001 --name verify-result
```

### 5. 如有 session，打开 provider 会话命令

如果对应 attempt 写出了 `worker-ref.json`，可以取回 provider 的继续命令：

```bash
cargo run -- run open-session task-001 run-001 --round round-001 --node accept --attempt attempt-001
```

对 `claude-code` provider，典型输出类似：

```bash
claude -c session-123
```

### 6. 如 run 暂停或需要重试

继续当前可恢复 run：

```bash
cargo run -- run continue task-001 run-001
```

重试当前节点的新 attempt：

```bash
cargo run -- run retry task-001 run-001
```

终止当前 run：

```bash
cargo run -- run kill task-001 run-001
```

## Inspecting outputs

人工验收时，至少检查这些文件是否符合预期：

- `.gold-band/tasks/task-001/runs/run-001/run.json`
- `.gold-band/tasks/task-001/runs/run-001/workflow.snapshot.json`
- `.gold-band/tasks/task-001/runs/run-001/rounds/round-001/round.json`
- `.gold-band/tasks/task-001/runs/run-001/rounds/round-001/nodes/dev/attempt-001/artifacts/exec-plan.json`
- `.gold-band/tasks/task-001/runs/run-001/rounds/round-001/nodes/run-tests/attempt-001/artifacts/exec-result.json`
- `.gold-band/tasks/task-001/runs/run-001/rounds/round-001/nodes/accept/attempt-001/artifacts/verify-result.json`

如果 provider 返回了会话信息，还会有：

- `.gold-band/tasks/task-001/runs/run-001/rounds/round-001/nodes/<worker-or-verify-node>/attempt-001/worker-ref.json`

## Troubleshooting

### `claude --version` 不通过

说明本机还不能走真实 `worker` / `verify` 端到端。先安装并配置 Claude Code CLI。

### `cargo test --quiet` 能过，但人工 E2E 跑不通

这是正常可能发生的情况：测试里大量使用 fake provider，而人工 E2E 依赖真实 `claude` CLI。

### 找不到 artifacts

先检查：

- 是否在 repo root 执行命令
- `task_id` / `run_id` / `round` / `node` / `attempt` 是否用的是实际值
- `.gold-band/tasks/...` 目录是否真的已经生成
- 当前 attempt 是否其实已经以 `failure` 结束，因此没有生成该节点预期的 artifact

例如一次真实人工测试里，`cargo run -- run start task-001` 的返回可能是：

- `status: "paused"`
- `pause_reason: "error-blocked"`

同时当前节点 `node.json` 可能显示：

- `status: "completed"`
- `outcome: "failure"`

这种情况下优先去看 `run status`、对应 attempt 的 `node.json`，以及是否真的存在 `worker-ref.json` / `artifacts/*.json`，而不要先假设一定是路径写错。

### `run open-session` 没有返回可执行命令

说明当前 attempt 没有保存可恢复 session，或者 provider 不支持 open-session。

## Useful source references

- CLI 命令定义：[src/cli/mod.rs](src/cli/mod.rs)
- runtime 路径定义：[src/storage/mod.rs](src/storage/mod.rs)
- Claude provider 实现：[src/provider/mod.rs](src/provider/mod.rs)
- happy path 测试：[tests/full_mvp_flow.rs](tests/full_mvp_flow.rs)
- acceptance / open-session / kill 测试：[tests/acceptance_loop_and_commands.rs](tests/acceptance_loop_and_commands.rs)
