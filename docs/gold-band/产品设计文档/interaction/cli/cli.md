# Gold Band CLI 规范

## 1. 一句话定义
Gold Band CLI 是**无人值守的 headless runner**：在当前 Git 仓库中以 AUTO 或指定工作流执行一次需求，前台跑到 run 结束或暂停，输出 canonical `RunState` 并以退出码表达结果；暂停的 run 可以用 `continue` 在无头环境中继续。

它服务于 benchmark、CI 等自动化场景，不是人工操作入口；人工操作统一在桌面客户端完成。

## 2. 设计原则
- **与桌面同一创建权威**：CLI 与桌面都通过 core `App::create_conversation_run` 创建会话任务与 run，task 元数据、AUTO 配置、Git preflight 与 run 生命周期完全一致；CLI 创建的会话可在桌面中打开、查看和继续。
- **交互模式是宿主属性**：CLI 以 `InteractionMode::Unattended` 构造 `RuntimeConfig`，不写入 run 状态；同一 run 在桌面中恢复后重新可交互。
- **只做一件事**：只有执行（`run`）与继续（`continue`）两个子命令，不提供 task/run/artifact 查询类子命令，结果以 stdout JSON 和退出码交付。

## 3. 命令

```bash
gold-band [--log-level <error|warn|info|debug|trace>] run \
  --requirement-file <requirement.md> \
  (--auto-config <auto.json> | --workflow <workflow.json>)
```

- 工作目录即目标仓库。AUTO、包含 AI-DYNAMIC 的工作流和 worktree 工作位置都必须是具有 HEAD 的 Git repository（既有 preflight）。
- `--requirement-file`：文件全文作为会话需求（user prompt）。
- `--auto-config` 与 `--workflow` 必须且只能选一个：
  - `--auto-config`：按 `ConversationAutoConfig` 反序列化的 JSON，例如固定 agent、模型与 `configOptions`（如 `{"effort":"max"}`）。
  - `--workflow`：与桌面工作流相同格式的 JSON，即 `{ workflow, modelBindings }`；也可以是 worker 节点直接写 `provider`、`model`、`permission_mode`、`auto_accept`、`config_options` 的裸 workflow，读取时自动迁移为按 execution slot 标识的绑定。入口与节点由文件决定，例如去掉可选入口节点并把 `entry` 指向后继节点。
  - 运行前，CLI 对绑定涉及的每个 Agent 运行一次 doctor 探测，结果只注入本次运行的内存诊断，再按桌面相同规则校验 Agent、模型、权限模式与选项；不可用或不支持时以结构化错误失败（退出码 3）。
- 用户级数据目录沿用 `GOLD_BAND_HOME` 与 settings/state 解析规则。
- task 来源标记为 `cli`（桌面为 `conversation-ui`）。

```bash
gold-band [--log-level <error|warn|info|debug|trace>] continue \
  --task-id <task_id> --run-id <run_id>
```

- 继续当前仓库中一个可显式继续的暂停 run（`process-interrupted` / `runtime-abnormal`），参数取自上次输出的 `RunState` 的 `task_id` 与 `id`；同样前台跑到 run 结束或再次暂停，输出与退出码同 `run`。
- 继续语义与桌面一致：当前节点是 AI-DYNAMIC 且有一个暂停的内部节点时，该节点在自己的 ACP 会话中以 runtime resume 继续，完成后 runtime 照常收集 artifact 并继续调度动态图；否则走 run 级 runtime continue。多个内部节点同时暂停时以命令失败返回（退出码 3），需在桌面中逐个继续。

## 4. 无人值守交互
没有应答者时，runtime 在等待前直接结算待处理交互，first-writer-wins，不阻塞 run：
- elicitation：以 `decline` 结算。
- permission：以 `cancelled` 结算。需要免确认执行时，应在 AUTO 配置或工作流绑定中选择对应 permission mode / Auto Accept，而不是依赖 CLI 代答。
- 暂停类节点（如人工 check、需要人工决策的 pause）不会被自动推进，run 以 Paused 返回。

## 5. 输出与退出码
stdout 输出最终 `RunState` JSON；日志写入 runtime log，错误写 stderr。

| 退出码 | 含义 |
| --- | --- |
| 0 | run Completed 且 outcome = Success |
| 1 | run Completed 但 outcome 非 Success（Failure / Killed 等） |
| 2 | run Paused，暂停原因见 `RunState` |
| 3 | 命令失败：参数、配置、Git preflight 或创建 run 出错，未产生可用 run |
| 4 | run 返回时仍为 Running（不应出现，作为防御性类别） |

## 相关文档
- [Agent 会话观测规范](progress.md)
