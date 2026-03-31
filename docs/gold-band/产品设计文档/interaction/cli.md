# Gold Band CLI 规范

## 1. 一句话定义
Gold Band CLI 是 Gold Band 的**核心 backend 接口**。

它既是：
- 用户直接使用的完整产品入口
- VSCode 插件调用的后台执行引擎
- runtime、provider 与 artifact 能力的外部统一接口

当前 CLI 包含两种等价入口：
- scriptable subcommand CLI：面向脚本、自动化、外部调用
- command-driven console CLI：面向人工控制台操作与可视化浏览

## 2. 设计原则

### 2.1 CLI 是一等公民
CLI 不是调试工具，而是完整产品入口。

### 2.2 VSCode 插件封装 CLI
插件应尽量通过调用 CLI 完成：
- 启动 run
- 查询 run 状态
- 查看事件与 artifact
- 控制验收失败后的继续执行或停止
- 打开原始 worker 会话

### 2.3 provider 与 profile 由 node / runtime 决定
- `worker` 节点可显式声明 `provider`
- 若节点未声明 `provider`，则由 runtime 使用系统内部默认 provider（当前 MVP 为 `claude-code`）
- `profile` 由节点声明的 profile 名解析得到，并按项目目录 > 用户目录查找

不建议用 `--agent` 同时表达这两层语义。

## 3. 顶层命令空间

scriptable CLI 入口：

```bash
gold-band task ...
gold-band run ...
gold-band artifact ...
gold-band inspect ...
gold-band provider ...
gold-band console
```

console CLI 入口：

```text
/task ...
/run ...
/artifact ...
/inspect ...
/provider ...
/help
```

约束：
- console CLI 的 slash command 与 scriptable CLI 共享同一套命令语义
- console CLI 前期不做自然语言解析
- `/run --help`、`/artifact --help` 等帮助以可视化方式展示，但不改变底层命令合约

## 4. 公共参数

当前 MVP 只保留与 run 选择和控制直接相关的公共参数。

当前已支持：
- `--log-level <error|warn|info|debug|trace>`：控制 runtime debug 日志级别

说明：
- 不暴露用户态 `--provider`
- 不暴露用户态 `--profile`
- provider 与 profile 的解析都在 runtime / node 层完成
- runtime 相关配置统一收敛到 `RuntimeConfig`，CLI 负责构造并注入 `App`

## 5. `task` 命令

```bash
gold-band task list
gold-band task show <task-id>
```

## 6. `run` 命令

```bash
gold-band run start <task-id>
gold-band run start <task-id> --workflow path/to/workflow.json
gold-band run status <run-id>
gold-band run events <run-id>
gold-band run continue <run-id>
gold-band run retry <run-id>
gold-band run kill <run-id>
gold-band run open-session <run-id> --round round-001 --node develop --attempt attempt-002
```

### `run start` 的 workflow 解析优先级
默认情况下，`run start <task-id>` 应从 task 解析默认 workflow。

建议优先级：
1. CLI 显式覆盖：`--workflow <path>`
2. task 目录下声明的默认 workflow
3. 项目目录下的预设 workflow
4. 用户目录下的预设 workflow

说明：
- 首版推荐 run 仍然必须归属于某个 task
- 即使使用 `--workflow` 覆盖，也应在该 task 下生成本次 run 的 workflow snapshot

### `run continue` / `run retry` / `run kill` 的语义
- `run continue <run-id>`：用于继续当前 attempt 或重新结算当前 attempt；它不新建 attempt
- `run retry <run-id>`：用于对当前 node 重新发起一次新的 attempt；它一定新建 attempt。典型用于 `worker.failure`，或用户决定放弃当前 `worker.invalid` attempt 后重新生成
- `run kill <run-id>`：用于显式结束当前 run，并将 run 以 `completed + killed` 终局语义落盘

其中 `run continue` 在 MVP 中分两类：
1. **resume current provider session**
   - 适用于当前 attempt 处于 `paused + process_interrupted`
   - runtime 应尝试恢复当前 attempt 对应的 provider 会话
   - 不新建 attempt，也不新建 round
2. **re-evaluate current attempt**
   - 适用于当前 attempt 处于 `completed + invalid`，且 run 处于 `paused + error_blocked`
   - runtime 只重新读取并校验当前 attempt 目录中的现有产物
   - 不重新调用 provider
   - 若当前产物已满足 contract，则继续自动流转；否则继续停在当前 node

补充说明：
- `run continue` / `run retry` 是 runtime 控制动作，不等同于 provider 的 `sessionMode = continue | new`
- `run continue` 只允许作用于“当前 attempt”；不支持跨 attempt 指定继续
- `run continue` 仍在 Gold Band 内完成控制判断；它可以触发 provider resume，但不把控制权交给 provider
- `run retry` 一定新建 attempt，且手动 `retry` 默认以 `session = new` 启动；只有 workflow edge 明确声明 `session = continue` 时，runtime 才应请求历史会话复用
- 对已满足成功条件的 attempt，runtime 应自动流转，不应要求用户再执行一次 `run continue`
- MVP 不提供用户显式 `pause` 命令；`paused` 只表示系统观测到的挂起态

### `open-session` 的语义
- attempt 的唯一定位最小集合为：`run-id + round-id + node-id + attempt-id`
- CLI 读取该 attempt 的 `worker-ref.json`
- 根据其中记录的：
  - `provider`
  - provider-specific 引用
  - 打开/继续命令模板
- 在新终端或新窗口中打开对应原始会话
- 这是把控制权交还给 provider 的 handoff；打开后不再由 Gold Band 持续托管该次交互
- 若 `supportsOpenSession = false`，CLI 必须明确报错，而不是静默降级为 `run continue`

## 7. `artifact` 命令

```bash
gold-band artifact list <run-id> --round round-001 --node develop --attempt attempt-002
gold-band artifact show <run-id> --round round-001 --node run-tests --attempt attempt-001 --name exec-result
gold-band artifact export <run-id> --round round-001 --node accept --attempt attempt-001 --name verify-result
```

## 8. `inspect` 命令

```bash
gold-band inspect task <task-id>
gold-band inspect run <run-id>
gold-band inspect node <run-id> --round round-001 --node accept --attempt attempt-001
```

## 9. `provider` 命令

```bash
gold-band provider list
gold-band provider show <provider>
gold-band provider doctor <provider>
gold-band provider test <provider>
```

## 10. 相关文档
- [Provider Adapter 接口](../provider/adapter.md)
- [Progress 规范](progress.md)
- [Worker Ref 规范](../provider/worker-ref.md)

## 11. 一句话总结

> CLI 是 Gold Band 的核心产品接口；它以 `task / run / artifact / inspect / provider` 为顶层命令空间，而具体使用哪个 provider、哪个 profile，则由节点配置与 runtime 解析规则共同决定。这里 `continue` 主要对应 paused 或 invalid attempt 的恢复/重结算，`retry` 对应新建 attempt。