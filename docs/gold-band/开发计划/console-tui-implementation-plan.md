# Gold Band Console TUI 实现计划

## 1. 目标
为 Gold Band 增加一个 workflow-first console CLI，同时保留现有 scriptable CLI 的自动化与 JSON/raw 输出能力。

## 2. 实施原则
- 不改变 runtime 语义
- 不引入自然语言解析
- 不破坏现有 CLI 输出兼容性
- 尽量复用 `App`、`GoldBandPaths`、observability 文件
- 交互层允许做破坏性重构，但 runtime / provider / persistence 边界保持稳定

## 3. 建议技术栈
- `clap`：保留现有 argv CLI
- `ratatui`：TUI 布局与组件
- `crossterm`：终端输入与事件
- `tokio`：异步刷新与调度

## 4. 模块拆分
当前重点模块：
- `src/command/mod.rs`
- `src/command/execute.rs`
- `src/console/mod.rs`
- `src/console/state.rs`
- `src/console/events.rs`
- `src/console/commands.rs`
- `src/console/controller.rs`
- `src/console/view_models.rs`

同步修改：
- `src/cli/mod.rs`
- `src/lib.rs`
- `src/app/mod.rs`
- `src/runtime/mod.rs`
- `src/inspect/mod.rs`

## 5. 当前阶段里程碑

### Milestone 1：task-aware read model
- 扩展 `task.json` 最小 metadata，补 `description`
- 增加 task summary / workflow validation / resumable run 只读聚合接口

### Milestone 2：screen 化 console shell
- 以 Welcome 为默认入口
- 加入 Task Picker
- 明确 Workspace screen
- 删除旧的 explorer-first 主心智

### Milestone 3：workflow DAG workspace
- 主区域改为 DAG
- 节点切换后进入 detail panel
- detail 内逐级展示 attempt / artifact / attachment
- 边上直接显示 `√ / × / ？`

### Milestone 4：全局辅助命令收敛
- `/task`
- `/log`
- `/config`
- `/continue`
- `/help`
- 其余 attempt / artifact 操作优先走 Enter / Esc 下钻，而不是命令

## 6. App 只读接口补齐
新增或强化：
- `task_summaries()`
- `task_summary(task_id)`
- `attachment_list(...)`
- `attachment_show(...)`
- `node_runtime_summary(...)`
- workflow validation / resumable run summary helpers

## 7. 测试策略
- 保持现有 runtime/integration tests 通过
- 重写 console parser 测试
- 重写 console Welcome / Task Picker / Workspace 状态测试
- 增加 DAG 节点切换与 `Esc` 返回测试
- 增加 task description 展示与 resumable run 恢复测试

## 8. 手工验收清单
- `gold-band console` 可启动
- 首屏为 Welcome
- 可以进入 Task Picker 并看到 task description
- 进入 Workspace 后可以看到 workflow DAG
- 选中 node 后详情区展示 attempts
- 继续进入 artifact / attachment 后可查看内容并 `Esc` 返回
- `/task`、`/log`、`/config`、`/continue` 行为正确
- 旧 runtime passthrough 命令仍可工作

## 9. 一句话总结

> 当前 console 的实现顺序应是：先补 task/read model，再切 screen，再把 workspace 改成 workflow DAG + detail 下钻，最后再收敛 slash command 与文档。
