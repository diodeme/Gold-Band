# Gold Band

Gold Band 是一个 Rust CLI/runtime + Tauri 桌面应用，用来把任务按 workflow 编排为统一的 `worker` 节点执行流程。项目仓库内的 `.gold-band/` 只保留项目级配置覆盖；task、run、ACP 会话、日志、artifacts 和 attachments 等过程状态持久化到用户目录下的 per-project runtime store。

## Current MVP scope

当前代码里已经实现的能力：

- 读取 task authoring 输入并启动 run
- 执行统一的 `worker` 节点（默认 provider 为 Claude Code ACP）
- 通过 `output` + `success_condition` 做 AI 输出验证
- 通过 `manual_check` 做人工结果确认
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
- runtime canonical truth 仍以 state files 与 artifacts 为准
- provider 输出在 UI 中优先展示 ACP 会话事件，raw frames 仅作为诊断入口

## Frontend status

当前主线包含 Tauri + React 桌面前端。前端工作流编辑器只生成统一 worker 节点；节点结果判定通过“不开启 / 人工 check / AI 输出验证”三种模式配置。

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

## Runtime layout

Gold Band 会把过程数据写到用户级项目 runtime store：

```text
~/.gold-band/projects/{project-id}/
  project.json
  logs/runtime.log
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
                    acp.session.json
                    acp.events.jsonl
                    artifacts/
                      <artifact-key>.json
                    attachments/
```

当前仓库的 `<repo>/.gold-band/` 只用于项目级 presets / config 覆盖，不存放 task 或 run 过程状态。

更完整的布局说明见 [docs/gold-band/产品设计文档/runtime/layout.md](docs/gold-band/产品设计文档/runtime/layout.md)。

## Manual end-to-end walkthrough

下面这条 walkthrough 对齐当前测试里的 happy path：[tests/full_mvp_flow.rs](tests/full_mvp_flow.rs)。

### 1. 准备最小 task 输入

创建 user project runtime 中的 task authoring 目录。`{project-id}` 由仓库绝对路径直接转义得到，采用类似 Claude Code 的可读目录名（例如 `D--Projects-code-ai-Gold-Band`），首次启动 Gold Band 后可在 `~/.gold-band/projects/*/project.json` 中查看。

```bash
mkdir -p ~/.gold-band/projects/{project-id}/tasks/task-001/authoring
```

写入 `task.json`：

```json
{"version":"0.1","id":"task-001"}
```

写入 `requirement.md`：

```md
Implement feature
```

写入 `workflow.json`：

```json
{
  "version": "0.1",
  "id": "full-flow",
  "entry": "dev",
  "control": { "max_attempts": 1, "max_rounds": 1 },
  "nodes": [
    {
      "id": "dev",
      "type": "worker",
      "provider": "claude-code",
      "profile": "developer",
      "goal": "Implement the requirement",
      "primary_artifact": "implementation-result"
    },
    {
      "id": "test",
      "type": "worker",
      "provider": "claude-code",
      "profile": "tester",
      "goal": "Check the implementation and return JSON with result and reason fields",
      "primary_artifact": "test-result",
      "output": {
        "kind": "json",
        "artifact": "test-result",
        "schema": { "result": "boolean", "reason": "String" }
      },
      "success_condition": { "expression": "$.result == true" }
    },
    {
      "id": "accept",
      "type": "worker",
      "provider": "claude-code",
      "profile": "acceptance",
      "goal": "Assess acceptance and return JSON with result and reason fields",
      "primary_artifact": "accept-result",
      "output": {
        "kind": "json",
        "artifact": "accept-result",
        "schema": { "result": "boolean", "reason": "String" }
      },
      "success_condition": { "expression": "$.result == true" }
    }
  ],
  "edges": [
    { "from": "dev", "to": "test", "on": "success" },
    { "from": "test", "to": "accept", "on": "success" },
    { "from": "test", "to": "dev", "on": "failure", "session": "continue" },
    { "from": "accept", "to": "$end", "on": "success" },
    { "from": "accept", "to": "$new-round", "on": "failure" }
  ]
}
```

保存到：

```text
~/.gold-band/projects/{project-id}/tasks/task-001/authoring/workflow.json
```

### 2. 启动 run

```bash
cargo run -- run start task-001
```

预期输出是一段 JSON，其中通常会包含：

- `id`
- `status`
- `outcome`
- `current_round`
- `current_node`
- `current_attempt`

### 3. 查看 run 状态

```bash
cargo run -- run status task-001 run-001
```

如果 happy path 跑通，预期最终会看到：

- `status: "completed"`
- `outcome: "success"`

如果真实 provider 没有顺利产出结果，也可能暂停在当前 worker 节点。

### 4. 查看 artifacts

```bash
cargo run -- artifact list task-001 run-001 --round round-001 --node dev --attempt attempt-001
cargo run -- artifact list task-001 run-001 --round round-001 --node test --attempt attempt-001
cargo run -- artifact list task-001 run-001 --round round-001 --node accept --attempt attempt-001
```

直接查看节点产物：

```bash
cargo run -- artifact show task-001 run-001 --round round-001 --node accept --attempt attempt-001 --name accept-result
```

### 5. 如有 session，打开 provider 会话命令

如果对应 attempt 写出了 `worker-ref.json`，可以取回 provider 的继续命令：

```bash
cargo run -- run open-session task-001 run-001 --round round-001 --node accept --attempt attempt-001
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

- `~/.gold-band/projects/{project-id}/tasks/task-001/runs/run-001/run.json`
- `~/.gold-band/projects/{project-id}/tasks/task-001/runs/run-001/workflow.snapshot.json`
- `~/.gold-band/projects/{project-id}/tasks/task-001/runs/run-001/rounds/round-001/round.json`
- `~/.gold-band/projects/{project-id}/tasks/task-001/runs/run-001/rounds/round-001/nodes/dev/attempt-001/artifacts/implementation-result.json`
- `~/.gold-band/projects/{project-id}/tasks/task-001/runs/run-001/rounds/round-001/nodes/test/attempt-001/artifacts/test-result.json`
- `~/.gold-band/projects/{project-id}/tasks/task-001/runs/run-001/rounds/round-001/nodes/accept/attempt-001/artifacts/accept-result.json`

如果 provider 返回了会话信息，还会有：

- `~/.gold-band/projects/{project-id}/tasks/task-001/runs/run-001/rounds/round-001/nodes/<worker-node>/attempt-001/worker-ref.json`

## Troubleshooting

### `claude --version` 不通过

说明本机还不能走真实 provider 端到端。先安装并配置 Claude Code CLI。

### `cargo test --quiet` 能过，但人工 E2E 跑不通

这是正常可能发生的情况：测试里大量使用 fake provider，而人工 E2E 依赖真实 `claude` CLI。

### 找不到 artifacts

先检查：

- 是否在 repo root 执行命令
- `task_id` / `run_id` / `round` / `node` / `attempt` 是否用的是实际值
- `~/.gold-band/projects/{project-id}/tasks/...` 目录是否真的已经生成
- 当前 attempt 是否其实已经以 `failure` 结束，因此没有生成该节点预期的 artifact

## Useful source references

- CLI 命令定义：[src/cli/mod.rs](src/cli/mod.rs)
- runtime 路径定义：[src/storage/mod.rs](src/storage/mod.rs)
- Claude provider 实现：[src/provider/mod.rs](src/provider/mod.rs)
- happy path 测试：[tests/full_mvp_flow.rs](tests/full_mvp_flow.rs)
- acceptance / open-session / kill 测试：[tests/acceptance_loop_and_commands.rs](tests/acceptance_loop_and_commands.rs)
