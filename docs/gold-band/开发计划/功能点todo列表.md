# Gold Band 功能点 Todo 列表

| 状态 | 概要 | 说明 | 方案链接 |
|---|---|---|---|
| 完成 | CLI 顶层命令骨架 | 已实现 `task / run / artifact` 三类顶层命令与统一 `RuntimeConfig` 注入，满足 CLI-first MVP 主入口。 | `docs/gold-band/产品设计文档/interaction/cli.md:31-52, 54-128` |
| 完成 | `task show` | 已支持按 task id 读取并输出 `task.json`。 | `docs/gold-band/产品设计文档/interaction/cli.md:54-60` |
| 待办 | `task list` | 设计文档定义了 task 列表能力，当前 CLI 仍未实现。 | `docs/gold-band/产品设计文档/interaction/cli.md:54-60` |
| 完成 | `run start` | 已支持读取 task workflow、校验 DSL、创建 run/round/attempt 并从 entry node 开始执行。 | `docs/gold-band/产品设计文档/interaction/cli.md:63-86`; `docs/gold-band/产品设计文档/product/overview.md:49-53`; `docs/gold-band/开发计划/gold-band-mvp-plan.md:255-287` |
| 完成 | `run status` | 已支持读取并校验 `run.json`，输出 run 当前状态。 | `docs/gold-band/产品设计文档/interaction/cli.md:66-72`; `docs/gold-band/产品设计文档/runtime/state/run.json.md:14-149` |
| 完成 | `run continue` | 已支持两类 continue：恢复被中断 provider 会话，或对 `completed + invalid` attempt 重新结算。 | `docs/gold-band/产品设计文档/interaction/cli.md:87-120`; `docs/gold-band/产品设计文档/runtime/control.md:337-360` |
| 完成 | `run retry` | 已支持对当前 node 新建 attempt 并重新执行，默认使用新 session。 | `docs/gold-band/产品设计文档/interaction/cli.md:87-120`; `docs/gold-band/产品设计文档/runtime/control.md:361-374` |
| 完成 | `run kill` | 已支持显式结束 run，并同步落盘 run/round/node 的 `killed` 终局状态。 | `docs/gold-band/产品设计文档/interaction/cli.md:87-120`; `docs/gold-band/产品设计文档/runtime/control.md:390-417` |
| 完成 | `run open-session` | 已支持从 `worker-ref.json` 解析 provider-native 打开命令，并在不支持时显式报错。 | `docs/gold-band/产品设计文档/interaction/cli.md:111-121`; `docs/gold-band/产品设计文档/provider/worker-ref.md:76-104` |
| 待办 | `run events` | 设计文档定义了 run 级 events 查询命令，当前 CLI 尚未实现。 | `docs/gold-band/产品设计文档/interaction/cli.md:66-72` |
| 完成 | `artifact list` | 已支持列出 attempt 下 canonical artifacts。 | `docs/gold-band/产品设计文档/interaction/cli.md:122-128`; `docs/gold-band/产品设计文档/runtime/layout.md:392-425` |
| 完成 | `artifact show` | 已支持读取 attempt 下指定 artifact 内容。 | `docs/gold-band/产品设计文档/interaction/cli.md:122-128`; `docs/gold-band/产品设计文档/runtime/layout.md:392-425` |
| 完成 | `artifact show-path` | 当前代码已额外支持按绝对路径直接查看 artifact 文件。 | `docs/gold-band/产品设计文档/interaction/cli.md:122-128` |
| 待办 | `artifact export` | 设计文档定义了 artifact 导出命令，当前 CLI 尚未实现。 | `docs/gold-band/产品设计文档/interaction/cli.md:122-128` |
| 待办 | `inspect` 命令空间 | 设计文档定义了 inspect 系列命令，当前 CLI 尚未暴露。 | `docs/gold-band/产品设计文档/interaction/cli.md:130-136` |
| 待办 | `provider` 命令空间 | 设计文档定义了 provider list/show/doctor/test，当前 CLI 尚未暴露。 | `docs/gold-band/产品设计文档/interaction/cli.md:138-145` |
| 完成 | Workflow DSL 基础解析 | 已实现 `WorkflowDsl`、节点/边模型与结构化反序列化。 | `docs/gold-band/产品设计文档/dsl/overview.md:1-999`; `docs/gold-band/开发计划/gold-band-mvp-plan.md:113-126` |
| 完成 | Workflow DSL 基本校验 | 已实现 version、entry、节点唯一性、edge 合法性、verify 节点唯一性等校验。 | `docs/gold-band/开发计划/gold-band-mvp-plan.md:113-126`; `docs/gold-band/产品设计文档/runtime/overview.md:43-65` |
| 完成 | `session=continue` DSL 约束 | 已对显式 `session=continue` 做 provider 能力边界校验，当前仅允许默认 provider。 | `docs/gold-band/产品设计文档/product/overview.md:66-70`; `docs/gold-band/产品设计文档/provider/adapter.md:119-127` |
| 完成 | `exec.planFrom` 合法性校验 | 已校验 `exec` 只能引用 worker 节点，且来源 worker 必须声明 `primaryArtifact=exec-plan`。 | `docs/gold-band/开发计划/gold-band-mvp-plan.md:194-204`; `docs/gold-band/产品设计文档/runtime/control.md:187-220` |
| 完成 | runtime 顶层目录布局 | 已实现 `.gold-band/` 下 logs、tasks、runs、rounds、attempt、artifacts、attachments、raw stream 等路径模型。 | `docs/gold-band/产品设计文档/runtime/layout.md:98-445` |
| 完成 | 用户级 / 项目级 profile 路径解析 | 已实现项目目录优先、用户目录兜底的 profile 文件定位。 | `docs/gold-band/产品设计文档/runtime/overview.md:57-65`; `docs/gold-band/产品设计文档/runtime/layout.md:153-175` |
| 待办 | workflow preset 解析链路 | 设计文档要求支持项目/用户预设 workflow，当前仅支持 task workflow 与 CLI `--workflow` 覆盖。 | `docs/gold-band/产品设计文档/interaction/cli.md:74-86`; `docs/gold-band/产品设计文档/runtime/layout.md:153-160` |
| 完成 | `run.json` 状态模型 | 已实现 run 级状态结构与一致性校验。 | `docs/gold-band/产品设计文档/runtime/state/run.json.md:14-149` |
| 完成 | `round.json` 状态模型 | 已实现 round 级状态结构与一致性校验。 | `docs/gold-band/开发计划/gold-band-mvp-plan.md:127-138`; `docs/gold-band/产品设计文档/runtime/control.md:254-289` |
| 完成 | `node.json` 状态模型 | 已实现 attempt 级状态结构、resolved config 快照与一致性校验。 | `docs/gold-band/产品设计文档/runtime/state/node.json.md:14-145` |
| 完成 | `worker-ref.json` 状态模型 | 已实现 provider-specific 会话引用结构与最小校验。 | `docs/gold-band/产品设计文档/provider/worker-ref.md:30-104` |
| 部分完成 | `task.json` 扩展元数据 | 当前已实现最小 `id/title/description`，满足 console task picker 展示；更完整 task 管理字段仍待后续补齐。 | `docs/gold-band/产品设计文档/runtime/state/task.json.md:1-999`; `docs/gold-band/产品设计文档/runtime/layout.md:200-214` |
| 完成 | workflow snapshot 落盘 | 已在 run 启动时落盘 `workflow.snapshot.json`。 | `docs/gold-band/开发计划/gold-band-mvp-plan.md:21-24, 131-138`; `docs/gold-band/产品设计文档/runtime/layout.md:217-250` |
| 完成 | resolved workflow / provenance 落盘 | 已在 task authoring 目录落盘 `workflow.resolved.json` 与 `provenance.json`。 | `docs/gold-band/产品设计文档/runtime/layout.md:203-210`; `docs/gold-band/产品设计文档/runtime/overview.md:62-65` |
| 完成 | Claude Code provider 默认实现 | 已实现默认 provider、doctor、worker 调用、sessionId 提取、open command 构建。 | `docs/gold-band/产品设计文档/provider/overview.md:20-46`; `docs/gold-band/产品设计文档/provider/implementations/claude-code.md:17-39, 147-238, 275-341` |
| 完成 | provider 能力暴露 | 已实现 `supports_open_session / supports_continue_session / supports_raw_stream` 能力模型。 | `docs/gold-band/产品设计文档/provider/adapter.md:27-141`; `docs/gold-band/产品设计文档/provider/implementations/claude-code.md:303-324` |
| 完成 | prompt bundle 渲染 | 已实现 system/user prompt 组织、冷数据索引、feedback summary 注入。 | `docs/gold-band/产品设计文档/provider/prompt-bundle.md:1-999`; `docs/gold-band/产品设计文档/provider/implementations/claude-code.md:71-145` |
| 完成 | worker 节点执行 | 已支持 `worker` 节点按 resolved config 调用 provider，并生成 primary artifact / worker-ref。 | `docs/gold-band/开发计划/gold-band-mvp-plan.md:264-270`; `docs/gold-band/产品设计文档/runtime/control.md:102-128` |
| 完成 | verify 节点执行 | 已支持 verify 通过 provider 通道运行，自动收集证据并产出 `verify-result`。 | `docs/gold-band/开发计划/gold-band-mvp-plan.md:278-282`; `docs/gold-band/产品设计文档/runtime/control.md:141-152` |
| 完成 | exec 节点执行 | 已支持读取 `exec-plan`、串行执行命令、生成 `exec-result`。 | `docs/gold-band/开发计划/gold-band-mvp-plan.md:272-276`; `docs/gold-band/开发计划/gold-band-mvp-plan.md:194-204` |
| 完成 | `exec-plan` 规范化落盘 | 已支持对 provider 返回的 `exec-plan` 做解析、校验与规范化写入。 | `docs/gold-band/开发计划/gold-band-mvp-plan.md:151-164`; `docs/gold-band/产品设计文档/runtime/layout.md:394-406` |
| 完成 | `exec-result` 规范化落盘 | 已支持 `exec-result` 生成、校验与 canonical 写入。 | `docs/gold-band/开发计划/gold-band-mvp-plan.md:151-164`; `docs/gold-band/产品设计文档/runtime/control.md:129-140` |
| 完成 | `verify-result` 规范化落盘 | 已支持 `verify-result` 解析、校验与 canonical 写入。 | `docs/gold-band/开发计划/gold-band-mvp-plan.md:151-164`; `docs/gold-band/产品设计文档/runtime/control.md:141-152` |
| 完成 | outcome/status 分离 | 已在 run/round/node 状态模型中实现 `status` 与 `outcome` 分离约束。 | `docs/gold-band/产品设计文档/runtime/overview.md:66-80`; `docs/gold-band/产品设计文档/runtime/control.md:62-98` |
| 完成 | 控制流决策引擎 | 已实现基于 node outcome 的 `TransitionToNode / OpenNewRound / PauseRun / CompleteRun` 决策。 | `docs/gold-band/开发计划/gold-band-mvp-plan.md:206-237`; `docs/gold-band/产品设计文档/runtime/control.md:155-180` |
| 完成 | repair loop | 已支持 `exec.failure` 经 edge 回退到 worker，并累计 `repair_loops_used`。 | `docs/gold-band/产品设计文档/runtime/control.md:186-235`; `docs/gold-band/开发计划/gold-band-mvp-plan.md:420-422, 442-444` |
| 部分完成 | `exec.invalid` 默认 repair 规则 | 已支持 `exec.invalid` 进入暂停阻塞，但尚未实现“无显式 edge 时默认回到 `planFrom` 且优先 continue”的默认策略。 | `docs/gold-band/产品设计文档/runtime/control.md:215-220, 412-413`; `docs/gold-band/开发计划/gold-band-mvp-plan.md:316-320` |
| 完成 | acceptance loop | 已支持 `verify.failure + auto_loop` 新建 round，并将最新 verify 反馈带回 entry。 | `docs/gold-band/产品设计文档/runtime/control.md:236-307, 414-416`; `docs/gold-band/开发计划/gold-band-mvp-plan.md:423-424, 447-449` |
| 完成 | `$end` 终止语义 | 已支持命中 `$end` 时按 success/failure 完成 run。 | `docs/gold-band/产品设计文档/runtime/control.md:176-181, 446-449`; `docs/gold-band/开发计划/gold-band-mvp-plan.md:403-405, 447-449` |
| 完成 | run 级 progress 快照 | 已实现 `run-progress.json` 最小快照写入。 | `docs/gold-band/产品设计文档/interaction/progress.md:58-110, 142-156`; `docs/gold-band/开发计划/可观测性plan.md:80-112` |
| 完成 | run 级 events 时间线 | 已实现 `events.jsonl` 写入与关键生命周期事件记录。 | `docs/gold-band/产品设计文档/interaction/progress.md:112-156`; `docs/gold-band/开发计划/可观测性plan.md:80-112` |
| 完成 | attempt 级 raw stream 归档 | 已实现 provider stdout/stderr 采集并落盘 `raw.stream.jsonl`。 | `docs/gold-band/产品设计文档/interaction/progress.md:40-57, 142-156`; `docs/gold-band/开发计划/可观测性plan.md:113-145` |
| 完成 | runtime debug 日志 | 已实现 `.gold-band/logs/runtime.log` 与 observability 初始化。 | `docs/gold-band/开发计划/可观测性plan.md:19-47, 162-176`; `docs/gold-band/产品设计文档/runtime/layout.md:132-144` |
| 待办 | `progress.events.jsonl` 规范化事件流 | 当前仅保留路径与设计占位，尚未完整实现 attempt 级规范化进度事件。 | `docs/gold-band/产品设计文档/interaction/progress.md:47-57, 112-156`; `docs/gold-band/开发计划/可观测性plan.md:113-145, 217-218` |
| 待办 | DSL loop 参数合法性校验 | 需补齐 `maxRepairLoops`、`maxAcceptanceLoops` 的正整数与边界校验。 | `docs/gold-band/产品设计文档/dsl/control.md:109-117, 137-145` |
| 待办 | 无 verify 场景的 acceptance 配置校验 | 需校验没有 `verify` 节点时，`onAcceptanceFailure` 不能作为有效控制配置单独存在。 | `docs/gold-band/产品设计文档/dsl/control.md:233-237, 346-348` |
| 待办 | 节点保留字与 `$end` 合法性校验 | 需补齐节点 id 保留字冲突校验，以及 `to = "$end"` 时仅允许 `success/failure` 的约束。 | `docs/gold-band/产品设计文档/dsl/control.md:44-53, 336-337, 325-330` |
| 待办 | `session=continue` 目标能力校验 | 需按目标节点/provider 校验 continue 能力，而不只做当前的基础限制。 | `docs/gold-band/产品设计文档/dsl/control.md:349-350`; `docs/gold-band/产品设计文档/provider/adapter.md:124-126` |
| 待办 | repair / continue 的 session lineage | 需明确并实现 repair/continue 时复用最新 worker attempt 会话引用的规则。 | `docs/gold-band/产品设计文档/runtime/control.md:199-202, 337-374` |
| 待办 | `node.json.resolvedConfig.sessionMode` 对齐 | 需在 attempt 元信息中记录实际启动的 `sessionMode`，保证 continue/new 语义可追溯。 | `docs/gold-band/产品设计文档/runtime/state/node.json.md:83-104`; `docs/gold-band/产品设计文档/runtime/control.md:337-374` |
| 待办 | verify 证据包边界完善 | 需补齐 verify 对当前 round 最新 `exec-result`、上游 worker primary artifact、显式暴露 attachments 的证据收集规则。 | `docs/gold-band/产品设计文档/provider/invocation.md:173-188, 206-213`; `docs/gold-band/产品设计文档/runtime/layout.md:408-425` |
| 待办 | cold attachments 暴露链路 | 需增加 attachments 的发现、筛选、暴露到 provider invocation 的完整链路。 | `docs/gold-band/产品设计文档/provider/invocation.md:206-213`; `docs/gold-band/产品设计文档/runtime/layout.md:408-425` |
| 待办 | prompt bundle 完整契约 | 需补齐 role contract、artifact schema prompt、cold attachment index 与更完整 runtime context 注入。 | `docs/gold-band/产品设计文档/provider/prompt-bundle.md:97-243` |
| 待办 | `worker-ref.json` 完整 schema 校验 | 当前仅有最小校验，仍需补齐字段、枚举、布尔类型、`openCommand` 结构等完整约束。 | `docs/gold-band/产品设计文档/provider/worker-ref.md:30-69` |
| 待办 | runtime 状态文件 schema 命名对齐 | 需统一 `task.json`、`run.json`、`node.json`、`worker-ref.json` 的落盘字段命名与文档 schema。 | `docs/gold-band/产品设计文档/runtime/state/task.json.md:15-43`; `docs/gold-band/产品设计文档/runtime/state/run.json.md:16-50`; `docs/gold-band/产品设计文档/runtime/state/node.json.md:16-53`; `docs/gold-band/产品设计文档/provider/worker-ref.md:30-45` |
| 待办 | `exec-plan.source.json` sidecar | `exec` attempt 目录仍需补齐 `exec-plan.source.json`，与 layout 设计保持一致。 | `docs/gold-band/产品设计文档/runtime/layout.md:339-355` |
| 待办 | `run-progress.json` stage 枚举对齐 | 需统一 `currentStage` 的命名格式与文档约定，并明确兼容策略。 | `docs/gold-band/产品设计文档/interaction/progress.md:95-104` |
| 待办 | `RuntimeConfig.default_provider` 生效链路 | 需让 runtime 默认 provider 真正由配置驱动，而不是只停留在配置结构中。 | `docs/gold-band/产品设计文档/runtime/overview.md:81-92` |
| 待办 | 用户目录 `~/.gold-band` 解析 | 需将用户级目录解析收敛为 home 语义，而非固定机器路径。 | `docs/gold-band/产品设计文档/runtime/layout.md:48-68` |
| 待办 | CLI 参数契约统一 | 需统一 `run/*`、`artifact/*` 命令的参数定位模型，并与文档示例保持一致。 | `docs/gold-band/产品设计文档/interaction/cli.md:61-72, 122-128` |
| 待办 | `open-session` handoff 语义收敛 | 需明确并实现 `run open-session` 是实际打开 provider 会话，还是只输出 provider-native command。 | `docs/gold-band/产品设计文档/interaction/cli.md:111-121`; `docs/gold-band/产品设计文档/provider/worker-ref.md:76-93` |
| 待办 | inspect / provider 输出 schema | `inspect` 与 `provider` 目前只有命令设计，仍需补齐输出结构与交互契约。 | `docs/gold-band/产品设计文档/interaction/cli.md:130-145` |
| 完成 | Tauri 2.x 桌面端 MVP 原型对齐 | 新增 `src-tauri/` 与 `web/`，以 Tauri commands 复用 Rust core，并已按 `interaction/app/原型` 对齐应用壳、任务列表 Task Preview、工作流 execution history、Round 三块工作台和设置页本地偏好控件。 | `docs/gold-band/产品设计文档/interaction/app/overview.md:1-999`; `docs/gold-band/开发计划/gold-band-mvp-plan.md:55-65` |
| 完成 | 桌面端浏览器调试 fallback | `web/src/api.ts` 在非 Tauri 浏览器环境使用 mock view model，支持用 `npm run web:dev` 直接检查任务列表、工作流、Round 详情和设置页布局；Tauri 环境仍使用真实 commands。 | `docs/gold-band/产品设计文档/interaction/app/overview.md:1-999`; `docs/gold-band/开发计划/gold-band-mvp-plan.md:55-66` |
| 完成 | 桌面端 workspace 选择与记忆 | 已支持启动时恢复最近 workspace、从 `src-tauri/` 向上识别项目根、通过原生目录选择器切换 workspace，并在左侧应用壳展示切换入口。 | `docs/gold-band/产品设计文档/interaction/app/shell.md:27-140`; `docs/gold-band/开发计划/gold-band-mvp-plan.md:54-63` |
| 完成 | 任务列表宽度与刷新反馈 | 任务列表已改为固定比例列宽并在单元格内截断；手动刷新保留低对比度局部反馈，后台自动刷新静默更新，避免刷新按钮和表格顶部出现品牌色闪烁；首次加载使用骨架屏；未实现动作以显式禁用按钮展示。 | `docs/gold-band/产品设计文档/interaction/app/task-list.md:84-177`; `docs/gold-band/开发计划/gold-band-mvp-plan.md:54-65` |
| 完成 | 桌面端 Tailwind/shadcn UI 重构 | 桌面端 UI 已从自定义全局 CSS 一次性迁移到 Tailwind CSS v4 + `shadcn@latest`，基础控件使用 shadcn/ui 生成组件，保留原有 IA 与运行操作合约。 | `docs/gold-band/产品设计文档/interaction/app/overview.md:138-167`; `docs/gold-band/开发计划/gold-band-mvp-plan.md:54-65` |
| 完成 | 桌面端任务编排三页 IA 收敛 | 任务详情并入任务工作流页，run 详情并入 workflow run 分组，Round 详情作为唯一执行详情页。 | `docs/gold-band/产品设计文档/interaction/app/overview.md:49-68`; `docs/gold-band/产品设计文档/interaction/app/task-workflow.md:1-185`; `docs/gold-band/产品设计文档/interaction/app/round-detail.md:1-234` |
| 完成 | 桌面端真实工作流图 | 工作流卡片列表已替换为 React Flow 节点-边画布：任务工作流页只读展示原始 workflow，Round 详情页保留节点选中、双击详情和右键菜单。 | `docs/gold-band/产品设计文档/interaction/app/overview.md:152-177`; `docs/gold-band/产品设计文档/interaction/app/task-workflow.md:169-181`; `docs/gold-band/产品设计文档/interaction/app/round-detail.md:218-232` |
| 完成 | 桌面端测试问题修复 | 已补齐 workflow control 展示、前后端 i18n、Round trace 真实路径图、Round 动态 Tabs、任务/工作流列表分页排序与统一滚动布局；工作流蓝图页 control 规则条与画布分层展示，并限制 GraphView 自动放大，避免少量节点撑满画布。 | `docs/gold-band/测试问题/测试问题.md:1-20`; `docs/gold-band/产品设计文档/interaction/app/task-list.md:172-189`; `docs/gold-band/产品设计文档/interaction/app/task-workflow.md:169-188`; `docs/gold-band/产品设计文档/interaction/app/round-detail.md:218-237`; `docs/gold-band/产品设计文档/runtime/state/round.json.md:14-112` |
| 完成 | 任务编排首页视觉与溢出修正 | 已收敛首页 summary cards、间距与工具条视觉层级，Task Preview 改为固定 header + 内部滚动正文，执行统计窄栏单列展示，避免底部统计贴边或超出卡片。 | `docs/gold-band/产品设计文档/interaction/app/task-list.md:65-190`; `docs/gold-band/产品设计文档/interaction/app/overview.md:162-173`; `docs/gold-band/开发计划/gold-band-mvp-plan.md:54-72` |
| 完成 | 任务列表抽屉式预览交互 | Task Preview 已改为 shadcn/ui Sheet 右侧抽屉：初始不打开，单击任务行滑出，单击其他任务行切换内容，单击非任务区域、Escape 或关闭按钮收回。 | `docs/gold-band/产品设计文档/interaction/app/task-list.md:139-215`; `docs/gold-band/产品设计文档/interaction/app/overview.md:175-184`; `docs/gold-band/开发计划/gold-band-mvp-plan.md:54-73` |
| 完成 | Round 详情抽屉式详情查看 | Round 详情页右侧 Detail Viewer 已改为 shadcn/ui Sheet 详情抽屉，释放工作图和信息流宽度；双击节点、右键详情/会话、点击信息流条目打开抽屉；固定时切换为右侧占位面板，主工作区自动收窄；信息流拆为上下文与运行记录，固定态不复用 Sheet Portal，并用 `contextNodeId` 统一避免全局详情打断 node 上下文。 | `docs/gold-band/产品设计文档/interaction/app/round-detail.md:23-249`; `docs/gold-band/产品设计文档/interaction/app/overview.md:186-195`; `docs/gold-band/开发计划/gold-band-mvp-plan.md:54-74` |
| 完成 | 工作流运行记录紧凑分组列表 | 工作流运行记录已从 Run/Round 混合表格改为紧凑分组列表：Run 行只保留当前 Round 与必要操作，Round 明细只保留状态、结果、当前节点和查看入口。 | `docs/gold-band/产品设计文档/interaction/app/task-workflow.md:90-187` |
| 完成 | 浏览器调试 Deep Link | 桌面 Web 调试模式已支持 `/tasks`、`/tasks/:taskId/workflow`、`/tasks/:taskId/runs/:runId/rounds/:roundId`、`/settings`，便于 agent-browser 直达页面验证。 | `docs/gold-band/产品设计文档/interaction/app/overview.md:195-207`; `docs/gold-band/开发计划/gold-band-mvp-plan.md:54-76` |
| 待办 | 多 provider 接入 | 当前仅实现 `claude-code`，其他 provider 仍未接入。 | `docs/gold-band/产品设计文档/provider/overview.md:23-28`; `docs/gold-band/开发计划/gold-band-mvp-plan.md:46-53` |
| 待办 | provider 命令级诊断入口 | provider `doctor()` 已有代码实现，但 CLI 侧尚未提供 list/show/doctor/test 命令。 | `docs/gold-band/产品设计文档/provider/adapter.md:27-46`; `docs/gold-band/产品设计文档/interaction/cli.md:138-145` |
| 待办 | 项目索引 `index.json` | 目录设计中定义了项目级索引，当前 storage 尚未落地。 | `docs/gold-band/产品设计文档/runtime/layout.md:109-150` |
| 待办 | 高级调度 / 多 run 并发 orchestration | 设计文档明确列为后续能力，当前未实现。 | `docs/gold-band/开发计划/gold-band-mvp-plan.md:46-53` |
