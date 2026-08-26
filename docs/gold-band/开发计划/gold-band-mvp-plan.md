# Gold Band Rust MVP 实现方案

## 2026-08-25：PostTurn finalize 控制协议与 system prompt 隔离

- 根因与实现：artifact 后置的两阶段设计正确，但实现为复用控制结果提取逻辑，在隐藏 finalize 前把 `PostTurnProjection` 临时改写成 `InlineControl`；system prompt 渲染又把 `InlineControl` 解释为需要展开完整 output contract，导致 artifact 名称、schema 与 success condition 同时进入隐藏 user prompt 和会话级 system prompt，右侧“系统提示”最终显示 finalize 契约，session load/resume 时还可能真实追加该契约。现保留 contract 的原始 `PostTurnProjection` identity，以既有 `RuntimeFinalize / RuntimeRepair` render mode 单独判定当前 turn 是否消费 artifact；完整契约只由隐藏 user prompt 承载，业务、finalize 和 repair 的稳定 system prompt 均不展开 PostTurn schema。真正首轮内联控制的 AI-DYNAMIC bootstrap 继续使用 `InlineControl`，行为不变。
- 验收：Provider 接口单测固定 PostTurn 业务、隐藏 finalize 与隐藏 repair 的 system prompt 一致且都不含 artifact 名称/schema，finalize user prompt 仍包含完整协议、上下文与禁止继续业务工作的约束，并确认 finalize/repair 仍启用控制结果提取；AI-DYNAMIC acceptance prompt 回归固定 finalize 不再把 `dynamic-node-completion` 或 `next.type` 投影到 system prompt。Provider 单元测试 35 项、prompt bundle 接口测试 30 项、普通 workflow `worker_bootstrap` 20 项、AI-DYNAMIC 集成测试 25 项全部通过，Rust 格式与差异检查通过。
- 性能与过度设计评审：复用现有 `OutputEmissionMode`、`UserPromptRenderMode`、PromptBundle 和 artifact 提取链路，不新增状态机、持久字段、依赖、缓存、队列、扫描或兼容分支。每个 provider turn 只增加一次 O(1) 枚举匹配，不改变 ACP I/O、prompt 体积上限、锁范围、恢复次数或渲染范围；同时减少 PostTurn finalize system prompt 中的重复 schema 字节，无需专项 benchmark。

## 2026-08-24：ACP 取消超时后严格恢复原 Provider 会话

- 根因与实现：cancel drain timeout 把“本地 live route/attached runtime 不可安全复用”错误等同为“Provider session identity 不可继续”，通过 `write_worker_ref(..., reusable=false)` 清除了 `continue_ref`；同时既有 attempt 的人工/队列 turn 虽已按 Continue 渲染 PromptBundle，Tauri command 却再次读取首次 worker mode，可能向 ACP client 传入 `New + continue_ref`，恢复失败后便可静默进入 `session/new`。本次拆开两类事实：timeout 继续 shutdown 并隔离本地 runtime，但 worker ref 始终保留原 `acpSessionId`；既有 attempt 的 Direct、停止后追问、节点完成后 non-runtime-controlled 追问、队列派发和 dynamic 人工追问统一以本次执行意图 `SessionMode::Continue` 调用 ACP。公共 prompt 入口校验 Continue 必须携带有效恢复引用，缺失时返回 `acp.session-restore-reference-missing`；恢复按 capability 选择 resume/load，禁止新建 session。首轮节点启动、工作流迁移、dynamic 首轮、显式 runtime resume 和 hidden finalize/repair 仍由 orchestrator 的既有 invocation 规则决定，不建立 command 特判。
- 验收：Rust 回归固定 `worker_ref.mode=New + continue_ref=Some` 的既有 attempt 用户 turn 仍解析为 Continue，并固定 Continue 缺少 Provider session identity 时返回 blocked 结构化错误；恢复能力选择继续沿用 resume 优先、load fallback、strict unsupported 不得 StartNew 的既有测试。取消尾部 chunk 投影由独立修改负责，本项不改变其 terminal watermark、quiet drain 或 10 秒 deadline。
- 性能与过度设计评审：复用现有 `SessionMode`、worker continue ref、attached runtime shutdown 与 capability-driven resume/load，不新增状态机、持久字段、缓存、队列、锁或网络等待。新增工作仅为 O(1) session identity 校验与既有 worker-ref 常数级写入，不扫描 Timeline/raw，不改变正常首轮、attached reuse 或流式热路径复杂度，无需专项 benchmark。

## 2026-08-24：ACP 取消尾部输出按 prompt terminal 收敛

- 根因与实现：早期 durable cancel 实现把 `CancelRequested` 同时当作控制意图和正文截断点，导致已经进入 session route、甚至位于 outbound cancel 之前的 text/thought/tool update 只写入 raw 而不进入 Timeline；后续已有的原 `session/prompt` response watermark、200 ms quiet drain 和统一 10 秒 deadline 实际已经提供了正确终态屏障，但旧过滤没有随设计演进删除。本次删除该旁路过滤，取消期间继续有界投影 terminal 收敛前的当前 turn 内容，cancelled 仍拥有终态优先级；deadline 内正常确认取消后继续保留 attached session，只有超时未收敛才隔离 live route。
- 验收：Rust 回归固定 `ProviderControl=CancelRequested` 时 terminal drain 仍观察并保留正文 chunk，同时沿用既有 response watermark 顺序、route generation 隔离、200 ms quiet drain、10 秒总 deadline、超时 cancelled outcome 和 cancel redelivery 测试；Direct、Workflow、AUTO、AI-DYNAMIC 与 NonRuntime follow-up 继续共用同一 ACP prompt 入口，不增加模式特判。
- 性能与过度设计评审：复用现有 pending response watermark、event pump 和 terminal drain，不新增状态、持久字段、缓存、队列、锁或跨线程事务，也不扫描 Timeline/raw。取消低频路径只为本来已经读取、解析并写 raw 的 terminal 前尾部事件补齐必要的 Timeline 投影，仍受 64 帧/25 ms 排空时间片、既有队列上限和统一 10 秒 deadline 约束；正常 prompt 与正常取消后的 session reuse 路径复杂度不变，无需专项 benchmark。

## 2026-08-19：品牌 Logo 统一替换

- 实现：将用户提供的 `gold-band-logo-final-v6-transparent.svg` 作为唯一品牌矢量源复制到 `web/public/logo.svg`，以同一路径生成 `src-tauri/icons/logo-source.svg` 的 2048 正方形投影，并通过 Tauri 官方 icon generator 重建 Windows、macOS、PNG、Android 与 iOS 图标；README 中英文头标改为直接引用 `web/public/logo.svg`。
- 验收：品牌资产契约测试固定 1254 正方形、6 条路径、无内嵌位图，Tauri 源与前端 Logo 使用相同 path 序列，并检查标题栏、品牌加载态、默认 Agent 图标、workspace 选择页、favicon 和 README 继续使用 canonical `/logo.svg`/`web/public/logo.svg`；相关前端测试、生产构建和浅色 / 深色浏览器视觉验证通过。
- 性能与过度设计评审：继续复用既有单一 `/logo.svg` 消费路径，不新增运行时依赖、状态、请求或缓存；透明 SVG 由浏览器原生渲染，平台图标仍由 Tauri 官方生成器产出，避免手工维护每个尺寸。README 中已烘焙旧 Logo 的功能截图不属于本次自动同步范围。

## 2026-08-19：标题栏品牌标识对齐

- 根因与实现：此前标题栏沿用横向 `36px × 24px` 品牌框；新的 canonical Logo 为正方形，Grid 内 `<img>` 的 intrinsic minimum size 又阻止其缩入内容区，导致实际绘制尺寸超出品牌框而产生视觉错位。品牌框收敛为 `24px × 24px` 正方形，图片以 `size-full + min-h-0 + min-w-0 + object-contain` 约束在其可用内容区内，产品名继续由同一 flex 行的垂直中心线对齐。
- 验收：标题栏契约测试固定正方形品牌框、canonical Logo 路径和可收缩图片约束；浏览器在浅色、深色及窄窗口下确认图形不越过边框，且与标题视觉中心一致。
- 性能与过度设计评审：这是现有布局约束的常数级 CSS 修正，不增加 DOM、状态、依赖、请求或重渲染；不为单一资源增加位移变量或运行时测量逻辑。

## 2026-08-19：Windows 任务栏图标透明边缘

- 根因与实现：Tauri 的 SVG rasterizer 在小尺寸平台 PNG 的抗锯齿边缘写入了低 Alpha 的白色 RGB matte；Windows 深色任务栏进行透明合成时会把这些像素显示为白色晕边，SVG 标题栏渲染不受影响。保留同一 SVG 品牌源，并校正已生成平台 PNG 与 Windows 多尺寸 `.ico` 的透明边缘颜色，避免白色 matte 进入最终打包资源。
- 验收：品牌资产契约测试固定任务栏 `32×32` PNG 不含低 Alpha 白色像素；重新生成后的 `.ico` 包含透明的 16、24、32、48、64、256 尺寸，Windows 任务栏深色背景不出现白色晕边。
- 性能与过度设计评审：后处理只在开发期生成资产时执行，逐像素线性扫描有限图标文件；运行时不增加依赖、状态、I/O、缓存或渲染成本。

## 2026-08-19：workspace ProjectId 统一与一次性迁移

- 根因：旧 `project_id` 只是路径 slug，存在字符折叠和路径截断碰撞；Runtime recovery 与 ACP command catalog 又以规范化路径 `workspace_key` 建立平行身份。这是 workspace canonical identity 的根本缺陷，不是调用点漏传作用域。现统一为 `{最多 70 位可读 slug}--{8 位 BLAKE3}`，完整上限 80，三项源参数只从 `configs/app-config.toml [projectIdentity]` 读取。
- 数据与迁移：`project_id` 成为目录、状态引用、缓存、Runtime recovery 与 Scheduler 阻塞集合的唯一 workspace 身份。`core.db` recovery schema v2 删除 `workspace_key`；`stateSchemaVersion=2` 重写 workspace、last、pin 和 run mode。启动迁移器预检查后执行旧目录 rename、manifest，以及 `run.json`、`worker-ref.json`、ACP session/snapshot、文件变更记录和 AI-DYNAMIC graph workspace locator 的结构化改写；普通会话 linked worktree 通过 Git common-dir 和 catalog 条件式 repair；再同步重建搜索投影，最后写入 `core_schema.workspace_identity=3`。已完成 v1/v2 的机器会补跑缺失步骤；raw/timeline/diagnostics 历史不改写，rename 后中断与重复启动均按当前磁盘事实继续，完成版本命中后不再扫描。
- 边界与失败：workspace 注册和恢复严格校验 manifest，`workspace.already-exists`、`workspace.project-id-collision` 与 `workspace.manifest-mismatch` 分开返回，不保留旧 ID alias、大小写兼容或按路径重算 fallback。项目目录整体移动时 Scheduler DB 字节原样保留，但不迁移 Scheduler 表/definition schema；Git repair 失败只产生逐项告警，不阻断桌面启动，后续使用/删除前按登记事实重试；单条损坏 AI-DYNAMIC graph 隔离，不阻断其它 workspace。
- 验收：ProjectId 配置/固定向量/长度、manifest、core v1→v2、recovery fencing、ACP catalog、目录与状态迁移、rename 后续跑、AI-DYNAMIC graph 与 projection、真实 linked-worktree 父目录移动/repair/重复 ensure/dirty 保留/remove、搜索重建、Scheduler DB 字节不变及 v3 marker 幂等定向测试通过；两个 Rust crate 构建与格式检查通过。全量 workspace 测试仍受仓库现有无关夹具缺少 `acp_storage_schema_version` 阻断，未执行前端验证。
- 性能与过度设计评审：日常只增加单路径规范化与 BLAKE3 的 O(path length) 成本；locator 复用一次 runtime 遍历并跳过 worktree checkout，Git 检查与历史普通 worktree 数量线性，后续仅在使用/删除低频边界检查，搜索 O(tasks + sessions) 重建只发生一次。复用现有配置、manifest、StateConfig、core schema、SQLite transaction、Git coordination lock 和原生 repair，没有新增长期 alias 表、双写、轮询、队列、无界缓存或第二套 workspace aggregate。

## 2026-08-19：workspace manifest 写放大与原子写入口收敛

- 根因：`project.json` 是低频、可重建的 workspace identity manifest，却由高频 `App::with_config / with_repo_root` 构造路径无条件原子重写；桌面 IPC、刷新、恢复和 scheduler 会反复构造 `App`，把普通读取放大为磁盘写入。Windows 上 21,297 个 `.project.json.*` 残留说明至少有同量级原子写尝试未完成提交或清理；由于旧实现吞掉错误，无法再区分 commit 失败、进程中断和清理失败的占比。这是写入生命周期边界错误，不是需要替换 `atomic-write-file` 的库缺陷。
- 数据与接口：`ProjectManifest` 增加反序列化和语义相等能力，`GoldBandPaths::provision_project_manifest()` 返回 `Written / Unchanged`，`validate_project_manifest()` 为恢复候选提供只读 identity 校验；统一的 `storage::atomic_write_file()` 只封装临时文件写入与 commit，并原样传播写入/commit 错误。桌面端在首次 workspace 注册、桌面 workspace 切换和应用升级后的 scheduler workspace 注册边界按内容 provision；`App` 构造、普通详情读取和恢复扫描不再写 manifest。
- 失败语义：manifest provision 的写入/commit I/O 失败统一按第 1、2、4、8…次记录聚合诊断，包含 project、manifest 路径、OS `ErrorKind`、raw OS error 和完整错误链；普通 `DesktopContext` 读取继续，显式注册/切换等写边界仍可返回该错误，Scheduler 对单个失败 workspace 隔离并跳过注册。已有非空 runtime 缺少 manifest、manifest 损坏或稳定 identity 不匹配在所有边界都返回结构化数据完整性错误，不得被普通读取分支吞掉。接受 `atomic-write-file` 在极少数 commit 失败/进程异常时可能残留临时文件。canonical `run / round / node` 写入仍通过同一 helper 的错误返回路径中止状态转换，只有二次临时文件清理失败可忽略。
- 验收：核心库和桌面端 `cargo check` 通过；定向单测固定 manifest 首次写入、相同内容不改 mtime、版本/路径内容变化重写、恢复候选只读校验、原子写 open 错误向上传播、`App` 构造不创建 `project.json`，以及桌面 manifest I/O 错误在显式写边界返回、在普通读取边界继续、完整性错误始终不被吞。按要求不执行全量回归和 UI/网页验证。
- 性能与过度设计评审：高频 `App` 构造由每次一次原子写降为零 manifest I/O；低频注册边界为一次 O(1) 小 JSON 读取/比较，只有内容变化才产生一次原子写。除版本化的一次性 identity 迁移外，不增加常驻 workspace 扫描、缓存、锁表、重试线程或临时文件清理器。现有 manifest identity 与 canonical lifecycle 已足够表达不变量，不新增第二套状态模型或依赖。

## 2026-08-18：内置验证与审查角色非阻塞边界补齐

- 根因：测试与审查角色已有前序产物读取和 git 工作区回退规则，但没有完整区分“验证结论失败”和“工作流节点阻塞”；环境限制、人工验收或开发节点缺少报告可能被错误解释为 BLOCKED。这是既有角色职责契约不完整，不是 Runtime 生命周期或数据模型缺陷。
- 实现：中英文测试 profile 明确环境问题或人工验收只需如实记录未执行项与证据缺口，不构成阻塞条件；中英文审查 profile 明确前序开发节点没有产出 `dev-report.md` 时继续以当前 git 工作区对应改动为准。继续复用现有 profile system prompt、工作区 diff 和报告格式，不新增 fallback 层或运行时特判。
- 验收：通过 `App::profiles()` 接口同时读取中英文内置测试、审查角色，固定非阻塞语义及工作区回退契约；提示词通过现有 Rust 编译期 `include_str!` 管理，中英文目录结构保持一致。
- 性能与过度设计评审：仅增加四处静态提示词文本和一项接口回归，不新增状态、持久字段、依赖、扫描、缓存、队列、锁或 I/O；每次调用只增加常量级 prompt 字符串长度，现有 canonical profile ID 和加载路径均不变，无需 benchmark。

## 2026-08-18：ACP 结构化终态优先收敛

- 根因：ACP `session/prompt` 的 JSON-RPC response、session update 和普通 Agent 文本属于不同语义通道；旧实现只按 `end_turn` 和文本候选结算，并在 artifact 输出策略替换时丢失了 terminal failure 观察，导致 Codex 已通过 `willRetry=false` 或 `threadStatus=systemError` 宣告失败后，Runtime 仍可能进入 finalize / repair。问题是 ACP 控制面终态没有进入统一生命周期分类，不是某条 429 文本或某个节点的特例。
- 数据与接口：在单次 `AcpRuntime` prompt 生命周期内维护 transient `AcpPromptTerminalState`，只观察 adapter 已声明的结构化 Codex terminal metadata；`session/prompt` JSON-RPC error 直接转换为结构化 `provider.acp-prompt-failed`。用户停止仍拥有最高终态优先级；否则 terminal failure 优先于 `end_turn`、文本和 artifact，并在 provider 边界统一清除 retry policy、固定为 `RecoveryMode::Manual`。
- 生命周期：RuntimeControlled 业务 turn 收到结构化 terminal failure 后不写 `finalizing` checkpoint、不进入 finalize；failure 出现在 finalize / repair 时保留已有 checkpoint，但当前 drive 不再继续 repair。普通 worker、AI-DYNAMIC leaf 和 acceptance 最终都收敛为可显式继续的 `Paused + RuntimeAbnormal`。只有尚未形成 provider terminal verdict 的 transport interruption、临时本地资源等 `Auto` 错误保留最多三次自动重试；artifact/schema 非法继续使用独立的最多三次 repair，耗尽后同样进入可继续的 `RuntimeAbnormal`。
- 现场验收：task-038 的 `dev-test` 为 `end_turn → threadStatus=systemError → 无 ID 429 普通文本`，立即以通用 `provider.acp-error` 暂停，未 finalize、repair 或 auto retry；`accept` 的 prompt 直接返回携带 `usageLimitExceeded` 的 JSON-RPC error，立即以精确 `provider.acp-prompt-failed` 暂停。两者控制状态一致；横幅详情差异来自前者结构化 metadata 只有通用 systemError、后者 RPC error 携带精确原因，Runtime 不从普通文本猜错误分类。用户普通对话、显式继续和用户停止的既有优先级保持不变。
- 验收与评审：接口回归固定 terminal failure 覆盖 `end_turn`、晚到 quiet-drain terminal 可见、prompt RPC error 为 Manual 且无 retry policy、mixed terminal 不产生 `runtime_auto_retry`，并复核“最终无稳定 ID”、repair 三次耗尽、RuntimeAbnormal 可继续及停止后退出 retry wait 的既有契约；ACP client 97 项、provider 29 项、`worker_bootstrap` 20 项和 2 项 lifecycle 定向测试全部通过。复用现有 `AcpPromptFailure`、`RuntimeErrorInfo`、provider classifier 和 pause state，不新增持久字段、依赖、队列、扫描或第二套状态机；每个 session update 仅做常数级 metadata 字段读取，锁范围、I/O、消息窗口和渲染范围均不变，无需专项 benchmark。

## 2026-08-18：ACP 共享连接初始化与提前停止收敛

- 根因：ACP 连接池按 `provider_id + workspace_root` 复用物理进程，但 `initialize` 的等待与缓存仍由单个 attempt runtime 拥有；停止 attempt 会丢弃已发送请求的 response，却不逐出可能已经初始化的连接，后续会话复用同一 PID 并再次 `initialize`，触发 `Already initialized`。同时前端把 leaf `current` 当成初始化活动授权，已暂停的空 timeline 因而永久显示“Agent 调起中”。这是连接所有权和 UI lifecycle 投影的设计缺陷，不是 Codex provider 特例。
- 数据与接口：物理 `AdapterConnection` 新增 `Uninitialized / Initializing / Initialized / Failed` 单次初始化事务并缓存 capabilities；RPC 在状态锁外执行，其他调用方通过 condition variable 消费同一结果。connection manager 以无持久 key cache 的 condition-variable gate 串行同 key 创建。初始化固定 60 秒有界，不观察 attempt cancel；成功后被取消 attempt 在 `session/new` 前退出。失败连接按 key、Arc identity 与 generation 安全逐出，未取消调用方最多重建重试一次；旧代失败不能关闭新代连接。
- 提前停止终结：补齐 `initialize` / session setup 阶段取消与 transport interruption 的公共终结边界。`run_prompt` 在 provider 尚未接受 `session/prompt` 时统一结算 retry、持久化 `latestTurnStatus=cancelled` 与结构化 stop reason，随后释放 attempt-local provider control，最后调用既有 `session_update` 发布 idle lifecycle；没有真实 ACP session 时 snapshot 不写 `sessionId` 且保持 `availability=unavailable`，不以 attempt id 伪造 session。Direct 首轮/追问、普通 Workflow、AUTO、AI-DYNAMIC leaf 与定时运行均通过同一 ACP provider 入口生效；Doctor/MCP 独立健康检查不属于业务会话停止契约。
- UI 与错误：空 timeline 的 pending 只消费详情 query、权威 runtime/ACP lifecycle 与本地发送活动，不再消费 `current`；暂停/current 且 query 已完成时恢复 normal composer。初始化/加载错误依次使用 runtime error、query error、session diagnostics，最后才显示 provider-neutral fallback，通用文案不再指向 Claude。
- 验收：Rust 接口测试固定并行 caller 只执行一次 initialize、取消 waiter 不打断共享握手、失败或 panic 唤醒等待者且不缓存、同 key 创建单航班、旧 generation 不匹配 replacement，以及关闭一个 established session 不影响另一个；生命周期矩阵新增 `cancel-requested/stopping -> cancelled/idle` 顺序回归，要求同一页面无需导航或新建任务即可恢复 normal composer。`acp::connection` 28 项、`acp::client` 98 项和 Web stopping/composer 63 项通过，root crate、desktop production check、TypeScript check 与 Web production build 均通过；Web build 使用独立临时输出目录，未覆盖既有产物。Tauri lifecycle test 已固化，但完整 Tauri test target 仍被工作区既有的旧 DTO/fixture 编译错误阻断，未将这些无关改动纳入本修复。
- 性能与过度设计评审：只增加每物理连接一个初始化状态 mutex/condvar 与 manager 内瞬时 in-flight key set/condvar；不新增持久字段、队列、轮询、全量扫描或第二套 lifecycle。初始化 RPC、进程启动均在状态锁和全局 connection map 锁外执行，同 key 才单航班，不同 workspace/provider 保持并行；提前取消只增加一次既有小型 snapshot 原子写入和一次 session update，均为 O(1) 低频终结操作，不扩大锁范围、订阅或渲染范围，无需专项 benchmark。

## 2026-08-17：会话侧栏工作空间分组间距收紧

- 根因：工作空间列表结构、sticky 标题和展开生命周期设计正确，但每个分组外层仍统一使用 16px 底部间距，折叠工作空间较多时产生了超过分组层级所需的连续空白。这是共享排版 token 偏松，不是单个工作空间或截图尺寸的特例。
- 实现：继续复用现有 React、Tailwind 与 shadcn `ScrollArea`，将所有工作空间分组的统一底部间距从 16px 收敛到 8px；不改变标题行高、会话行高、展开内容、sticky 接替或“添加工作空间”入口。
- 验收：DOM 组件测试通过稳定的 workspace group 标记固定所有分组消费紧凑间距 token，视觉层级契约同时禁止回退到旧间距；2 个定向 Vitest 文件共 7 项测试通过，TypeScript 与 Web 生产构建通过。内置浏览器 deep link 在 1280px、720px、重新拉宽到 1440px 三种宽度下确认折叠/展开分组的计算间距均为 8px，无横向溢出或控制台告警；多工作空间一致性由包含 2 个 workspace 的 DOM 契约固定。
- 性能与过度设计评审：只替换一个共享 Tailwind spacing utility，并增加测试标记；不新增状态、effect、DOM 测量、依赖、缓存、请求或渲染分支。工作空间列表仍是既有单次 O(n) React 映射，DOM 数量与重渲染范围不变，无需 benchmark。

## 2026-08-16：产品悬浮提示统一迁移

- 根因：项目已经确立 shadcn/Radix Tooltip 与全局 Provider，但多个页面仍直接使用浏览器 `title`，React Flow 画布控制和 Streamdown 代码/图片控制还会由依赖内部间接生成 `title`。这是共享交互契约覆盖不完整，不是工作流或文件列表的局部样式问题。
- 实现：删除业务控件上的原生提示，统一组合现有 Tooltip；用 React Flow 官方 `Controls/ControlButton` 和实例接口封装共享画布控制，用 Streamdown 官方 `CodeBlock/CodeBlockCopyButton` 与 `components` 扩展点接管 Markdown 代码、图片控制，不修改 `node_modules`、不增加依赖、DOM 清理器或兼容分支。迁移范围包括标题栏、工作流、会话/ACP、轮次变更、附件、文件/源码管理、运行模式、定时任务及 Markdown 控制。
- 接口验收：增加源码 AST 契约，禁止原生标签及基础 `Button/Handle` 回退到 `title`；补充 prompt-kit action、轮次文件/运行产物、Markdown 代码复制与图片下载的服务端 DOM 测试。按用户要求不执行浏览器或桌面端人工验证，由用户根据最终验证点清单验收视觉位置。
- 性能与过度设计评审：Tooltip 仅在现有有限控件处增加有界组件树，不新增请求、缓存、队列、全量扫描或宽泛状态订阅；画布操作继续调用既有 React Flow 实例，复杂度 O(1)。Markdown 图片只保留依赖原有的单图片加载状态与点击下载 I/O，不增加轮询或观察器；现有 canonical 数据、生命周期和接口均无需扩展。
- 视觉回归修正：React Flow 的基础样式会对 `ControlButton` 下所有 SVG 强制 `fill: currentColor`，迁移后覆盖了 Lucide 线性图标的 `fill="none"`，使放大、缩小的 `+ / −` 被实心镜片遮蔽。现于共享 `.workflow-graph` 控制样式边界统一恢复 `fill: none / stroke: currentColor`，同时覆盖编辑态与运行态画布；契约测试禁止该边界回退为实心填充。本修正仅改变三个固定 SVG 的绘制属性，不新增渲染、状态、I/O 或主题特判。

## 2026-08-16：会话标题编辑宽度收敛

- 根因：共享可编辑标题组件把页头传入的 `flex-1` 布局职责直接附加到展示态 Tooltip trigger 和编辑输入框，导致视觉内容很短时，空白剩余区域仍可触发“修改标题”，编辑态也会占满整行。这是布局槽位与真实交互命中区没有分层，不是某个标题或截图宽度的特例。
- 实现：继续复用现有 React、Tailwind 与 shadcn Tooltip，不引入新组件或依赖。展示态与编辑态都增加只负责页头布局的外层槽位；Tooltip/click trigger 使用 intrinsic `inline-flex`，输入框使用浏览器原生 `field-sizing: content`，两者均通过 `max-w-full` 受槽位约束；同步补充输入框无障碍名称。
- 验收：DOM 单元测试固定父级 `flex-1` 只作用于布局槽位，展示态 trigger 与编辑态输入框均不继承伸展宽度且保留最大宽度约束；执行定向 Vitest、TypeScript 与 Web 生产构建，并使用内置浏览器 deep link 检查短标题、标题后空白命中、长标题与窄窗口编辑态。
- 性能与过度设计评审：没有新增 React state、effect、DOM 测量、ResizeObserver、缓存、I/O、依赖或逐帧计算；每次输入仍只更新标题组件自身，宽度由浏览器既有布局阶段计算。现有 canonical title、保存接口和事件链足以表达需求，不新增状态模型或抽象。

## 2026-08-17：字体栈与界面语言解耦

- 根因：Theme Contract 同时保存固定 `defaultFaces` 和 `byLocale / byScript` 分支，外观 resolver 又把界面语言作为字体 family 的投影输入；切到英文后 MiSans 被从全局栈删除，仍存在的中文任务名、文件名和消息便回退到系统字体。问题来自主题字体权威模型错误，不是侧栏字号、字重或局部样式不一致。
- 数据与实现：破坏式删除 Theme SDK、Web Zod、Rust serde 与主题包中的 `byLocale / byScript`，主题 `defaultFaces` 成为唯一默认有序栈；两个内置主题固定为 `Inter Variable → Gold Band MiSans → 系统 CJK fallback → sans-serif`。`resolveAppearance / applyAppearance` 删除语言参数，App 与 Settings 的语言事件不再重新应用外观或个性化字体；设置页通过独立 helper 读取 stack 的本地化 `displayName`。用户 `custom` 栈继续是唯一覆盖来源，浏览器原生 glyph fallback 继续负责混排。
- 验收：Theme SDK 拒绝重新声明语言分支并固定生成 CSS 的 Inter/MiSans 顺序；Vitest 固定主题 resolver 的完整 UI 栈及 UI/editor 隔离，Rust Catalog 固定两个内置主题的 `defaultFaces`；执行主题构建、定向单元测试、TypeScript、Web 生产构建和 Rust 定向测试，并在内置浏览器 deep link 中比较中英文切换前后的根变量与侧栏中文标题 computed `font-family`。
- 性能与过度设计评审：删除两类条件分支、locale 解析和语言变化触发的根变量重写，不新增状态、Context、依赖、缓存、队列、扫描或逐字符 JavaScript。MiSans 仍由浏览器在内容实际需要中文 glyph 时命中；已有固定有序栈和用户覆盖模型足以表达不变量，无需新增字体状态机或兼容层。

## 2026-08-15：会话侧边栏固定区与滚动区收敛

- 根因：侧栏把置顶区放在 workspace `ScrollArea` 之外，导致置顶会话增长时持续挤占 workspace 可视高度；同时“置顶”仍消费辅助 `text-xs`，与已经统一为 UI 基准字号的功能入口不一致。问题来自滚动容器边界和排版 token 未同步完成，不是单个截图尺寸下的间距问题。
- 实现：继续复用现有 shadcn `ScrollArea` 和 Tailwind token，把快捷/功能入口明确为固定导航区，将置顶区与 workspace 区纳入唯一会话滚动区，设置入口继续固定在底部；展开后置顶标题使用原生 CSS sticky，并由父级置顶容器限定吸附范围，使首个 workspace sticky 标题到达时自然接替。“置顶”与“定时任务”统一使用 `text-sm` 和 `sidebar-foreground`，不再让置顶标题降级为辅助 `muted-foreground`。顶部入口按钮高度、组内 gap、分隔线 margin 和侧栏纵向 padding 各收紧一档。定时任务入口同时改为消费现有中英文 i18n 文案。
- 验收：组件布局契约固定“固定导航在滚动区之外、置顶与 workspace 同处滚动区”、置顶标题 sticky、置顶/定时任务字号与前景色和顶部紧凑 token；执行定向 Vitest、TypeScript 与 Web 生产构建，并使用内置浏览器 deep link 到会话页检查正常/窄窗口、长列表滚动、置顶到 workspace 标题接替和固定区边界，同时在浅色、深色主题核对置顶与功能入口的计算颜色一致。
- 性能与过度设计评审：仅重排既有 DOM 容器并修改静态 utility class，不新增 React state、Context、滚动监听、缓存、队列、I/O、依赖或数据扫描；会话节点数量与渲染范围不变，滚动仍由 Radix/CSS 处理。现有组件足以表达需求，无新增抽象或假设性机制。

## 2026-08-14：会话页文字层级与有效留白收敛

- 根因：会话页的信息层级存在反转：workspace 分组标题小于其下会话项，顶部会话标题又大于正文；更根本的问题是消息、统计和 composer 横向铺满大屏剩余空间，阅读起点与行长随窗口无界增长。这是排版语义与内容宽度边界缺失，不是单个字号问题。
- 实现：不删除信息、不改变入口和交互，基于现有 Tailwind/shadcn/prompt-kit 组件建立稳定层级：workspace 标题、侧栏会话标题与顶部会话标题统一消费主题 `text-sm` token（默认 14px），重命名输入保持同字号；相对时间和 run ID 使用元信息字号。置顶区与 workspace 区的会话间距统一收敛为 Tailwind `space-y-0.5`（默认 2px），行内 padding 与点击热区保持不变。消息、运行统计和 composer 共用居中 56rem 阅读轨道，窄窗口自动退化为全宽并保留 20px 安全边距。消息间距保持 20px，侧栏继续用局部 margin 区分导航、置顶和 workspace。
- 排版补充：workspace 标题、当前会话与普通会话分别消费主题 `font-semibold / font-medium / font-normal`，当前 variable font 映射为 450 / 380 / 330；普通与选中会话标题统一使用完整 `sidebar-foreground`，不再用 85% 透明度制造额外的视觉变细，元信息继续使用 normal 字重与弱化色。视觉层级不只依赖字号，也不在组件中硬编码轴值。Streamdown 反引号行内内容改为与正文相同的 UI 字体、字号、字重和行高，只保留标签底色、圆角与内边距；fenced code block 继续使用等宽字体和 Shiki 高亮。
- 验收：新增会话视觉层级契约测试并收紧侧栏、Streamdown 样式契约；契约固定侧栏会话标题与重命名输入使用主题 `text-sm`、两个会话列表使用 `space-y-0.5`，并禁止回退为任意像素字号。内置浏览器在正常与窄宽度下验证消息轨道、侧栏和 composer 无横向溢出，并核对主题字号变化后会话标题继续跟随。行内代码的同字体、同字号、同字重、同上下文行高及 fenced code block 隔离由组件单元测试固化。
- 性能与复杂度评审：仅修改静态 class 与既有 DOM 的排版，不新增 React state、Context、订阅、I/O、依赖、缓存或逐帧计算，不扩大数据加载和渲染范围；继续复用现有组件，无过度抽象，性能风险可忽略。

## 2026-08-14：UI 小字号与文字颜色 class 合并修复

- 根因：全局 `cn()` 直接使用 `tailwind-merge` 默认规则，默认规则无法判断项目自定义的 `text-ui-nano / micro / caption / compact` 属于字号，把它们误归入文字颜色组；当 Button、Badge、CommandItem 或条件 class 同时提供文字颜色时，小字号被删除并回退到组件基础字号，反向顺序还可能删除组件语义颜色。
- 实现：在唯一 `cn()` 入口通过 `extendTailwindMerge` 把四个 UI 排版 token 注册到 `font-size` class group。业务组件不增加局部覆盖、不调整 class 顺序，现有和后续 shadcn/prompt-kit 消费路径统一恢复字号与颜色的正交合并。
- 验收：新增接口单元测试覆盖四个 UI 字号与透明文字颜色共存、标准字号和 UI 字号按后写值覆盖、hover/dark 状态颜色与基础字号共存；继续执行 Web 类型检查、生产构建，并从桌面 WebView 读取会话头按钮的最终 class 与 computed style。
- 性能与复杂度评审：只在模块初始化时创建一个扩展合并器，每次 `cn()` 增加四个静态候选匹配；不增加 React 状态、订阅、渲染、I/O、缓存或依赖，复杂度仍与 class token 数线性相关，无可感知性能风险，也没有为单个按钮引入补丁式分支。

## 2026-08-14：会话配置菜单选择生命周期修正

- 根因：共享 ACP 单项配置菜单错误复用了复合菜单的选择保持打开策略；PromptInput 的交互后代识别只包含普通 `menuitem`，遗漏 Radix 单选项实际使用的 `menuitemradio`，Portal 点击冒泡后被误判为空白点击并聚焦输入框。
- 实现：快速对话与会话详情继续共用 shadcn/Radix ACP 选择器；仅在 Agent 同时提供模型与 `category=thought_level` 时进入复合菜单并保持打开，纯模型与权限单项菜单恢复选择即关闭。PromptInput 完整识别普通、复选和单选菜单角色，选择配置不再抢占输入焦点。
- 验收：Vitest 固化单项/复合菜单分流和菜单角色焦点边界；按用户要求不启动前端、不执行浏览器或桌面交互验证。
- 性能与复杂度评审：能力判断沿用已有一次常量级分支，单项菜单删除受控开合 state、ref 与 timer，不增加订阅、缓存、I/O、全量扫描或渲染范围；复用现有组件与 Radix 默认生命周期，无过度设计。

## 2026-08-12：ACP 会话追问草稿运行期记忆

- 根因：运行中追问的正文与附件此前由 `ACPChatDialog` 本地 state 持有，而会话/节点切换会按 session key 重建组件；状态生命周期短于业务草稿生命周期，导致未发送内容丢失。
- 数据与接口：新增进程内 ACP composer draft store，以完整 session/event-window locator 为键统一保存正文与附件；React hook 只暴露当前 locator 的 `draft / setContent / setAttachments`，现有 prompt-kit composer 和附件选择器继续消费该接口。
- 快速对话 composer 的 workspace 切换沿用 App 层草稿边界：composer 选择器与左侧 workspace“新会话”入口都只切换 workspace 上下文，不清空未提交正文或附件；仅创建成功或用户明确清空/放弃时 reset，并以 Vitest 契约固定两条入口一致。
- 生命周期：普通切换保留，发送或明确清空删除对应内容，应用 `pagehide` 统一释放；不接入任何 durable storage，因此重启不恢复。store 限制 64 个草稿和 100 MiB 附件总量，LRU 淘汰同步释放 object URL。
- 验收：单元测试固定跨会话恢复与隔离、发送清空、容量淘汰和退出释放；执行定向 Web 测试、TypeScript/生产构建，并以 `/chat` deep link 验证文字和附件切换恢复。
- 性能评审：正文输入只更新当前 composer hook 与 O(1) Map 条目，不进入页面壳或历史消息订阅；附件总量有界，无全量历史扫描、I/O、请求、持久化、队列或后台轮询。容量检查最多扫描 64 个小草稿元数据，且只在草稿写入时发生。

## 目标

先实现一条最小可用闭环：

1. 读取 task + workflow
2. 跑 `worker`
3. 若产出 `节点输出产物`，跑 `worker`
4. 若有 `worker`，跑 `worker`
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
- `worker` 串行执行命令
- `worker` 调用 Claude Code
- canonical artifact 落盘
  - `节点输出产物`
  - `节点输出产物`
  - `验收输出产物`
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
- 非 ACP provider 的长期独立可视化协议
- `progress.events` 精细事件模型（已被 ACP-first 会话可视化方向取代）
- raw stream 复杂映射（后续只作为 raw/debug viewer）
- VSCode 插件
- 复杂 doctor/test matrix
- 高级调度 / 多 run 并发 orchestration

### 桌面端 MVP 增量
- 2026-08-13：完成“默认轻量工作流”。保留稳定 ID `default` 并将展示名调整为“默认完整工作流”，新增 `default-lightweight`，拓扑为 `grill -> dev-test -> accept`；新增内置 `pf-builtin-dev-test` 中英文角色 prompt。轻量模板验收失败通过 `$new-round(new_round_entry=dev-test)` 回到开发测试；完整与轻量模板都默认配置 `max_attempts=10`、`max_rounds=3`，重试和新 Round 次数统一遵循现有 Control DSL。原 `includeInterview` 特判已删除，改为模板元数据驱动的可选入口能力；模板只用 `isBuiltIn` 区分是否内置，不定义完整/轻量类型枚举。完整模板显示采访开关，轻量模板显示拷问开关，偏好按 workspace/template 持久化，定时任务冻结创建时的有效选择。Rust 编译与专项接口测试、Web 全量测试、生产构建及 `/chat`、`/chat/run-modes` 页面验收通过；根 crate 全量 Rust 测试在 10 分钟工具窗口内未结束且无失败输出，已在实施方案中如实记录。详细数据、接口、测试与性能结论见 `docs/gold-band/开发计划/新增流程/默认轻量工作流实施方案.md`。
- 2026-08-12：完成 Workflow Runtime execution 与 ACP 生命周期解耦。`run.json.execution` 以显式 phase、精确 locator 和单调 revision 成为 Workflow/AUTO 阶段唯一权威源；Runtime control、ACP session availability、进程内 live turn 与 latest turn 历史分别投影。破坏式删除 `runtime active + ACP terminal => launching-next-node` 及通用 ACP active/terminal DTO 消费，`acp.snapshot.json / acp.session.json` 也从混合 `status` 迁移为 `availability + latestTurnStatus`，旧文件首次读取后一次性回写。停止后的 NonRuntime 追问结束仍保持 Paused，继续命令在后台启动前先提交 checkpoint phase。`run-progress.json` 仅作 revision 对齐后的观测，启动恢复仍统一收敛为 `Paused + ProcessInterrupted`。Rust/Web 接口回归覆盖停止/恢复、manual check、Direct、AI-DYNAMIC、stale snapshot/progress、metadata migration 与 sidebar/composer 单调收敛；不增加轮询、timeline 扫描或 token 热路径写入。
- 2026-08-10：完成 AI-DYNAMIC 工作空间树与 Git 基础设施 V2。破坏式删除 Agent-facing `WorkspaceMode / WorkspacePolicy`，runtime 以 `WorkspaceState` catalog 统一管理 main/worktree 的身份、父子关系、所有权和生命周期；`single` 继承来源 workspace，`fanout` 自动从来源 workspace checkpoint 分叉隔离 worktree，嵌套 fanout 的 merge/acceptance 回到 `group.targetWorkspaceId`。新增基于 Git CLI 的 typed `GitRepositoryService / GitWorkspaceManager`，供 runtime 与后续右侧 Git 面板共用；AUTO 和含 AI-DYNAMIC 的固定工作流在创建 run 前执行 Git/仓库/HEAD/worktree preflight，桌面端用 shadcn 对话框支持下载 Git、重新检测、初始化仓库或切换工作流。Rust 接口测试固化 preflight、checkpoint、single 继承、fanout 隔离和嵌套父 workspace 路由；后续右侧 Git 状态/提交面板继续复用该服务边界，不进入本次 UI 范围。
- 2026-08-07：补齐内置角色元数据国际化。内置角色名称、摘要与正文统一按 `desktop_language` 选择中文或英文版本；`pf-builtin-*` profile ID、默认工作流 DSL、任务 workflow 和运行快照中的角色引用保持不变。Rust 单元测试覆盖全部内置角色的中英文名称、摘要差异与 ID 稳定性。
- 2026-08-06：修正 AI-DYNAMIC 动态策略的控制面边界。AUTO 与 Workflow 依次配置初始分发 Agent、分发模型、验收模型和共享原生权限；验收模型目录只读取初始分发 Agent，不再聚合候选 worker Agent。bootstrap、merge、acceptance 固定使用该 Agent 并共用权限，只有 worker 由 proposal 选择 provider；output contract 禁止 merge / acceptance 输出 provider，并继续禁止模型输出 model/permissionMode。删除过渡字段 `bootstrapPermissionMode`，统一使用 dynamic strategy 的 `permissionMode`，候选 worker 仍各自保存模型与权限。
- 2026-08-06：会话侧边栏进行中状态收敛为既有视觉载体的低强度呼吸动画：Workflow/AUTO run 为 `gold-running` 蓝色圆点，Direct 为现有 Agent icon；移除 Direct 图标外围旋转圈。暂停黄色、成功绿色、失败红色及其状态语义完全不变；所有动画通过 `motion-safe` 尊重 reduced-motion。Vitest 固化运行中蓝色动画与其余终态颜色不变的接口契约。
- 2026-08-06：将 ACP 长文本流的上限检查与有界字符串追加从重复扫描历史字符串改为增量字符计数。prompt visible/recent-message 输出投影和 canonical timeline text/thought/plan stream 在自身生命周期内维护字符数，每个新 chunk 只扫描一次，该累计步骤由最坏 O(n²) 收敛为 O(n)；保留原累计快照、字符上限、Unicode 截断和消息展示行为，不调整 artifact 内存策略。Rust 回归覆盖一万个单字符 chunk、中文/emoji 与达到上限后的幂等追加。
- 2026-08-06（历史方案，artifact 分类已于 2026-08-18 替换）：完成 ACP 无 `messageId` agent 输出的通用展示与 contract 分类。当时移除 Codex adapter 私有过滤，所有无 ID `agent_message_chunk` 均进入 canonical timeline 和 Agent 消息气泡；`output_contract=None` 的 Direct/对话调用按 `Conversation` 宽松策略正常结束，Workflow / Auto / AI-DYNAMIC 等存在 `output_contract` 的调用则按 `ArtifactContract` 严格策略仅消费有 ID 输出。该版的 `provider.acp-unidentified-agent-output` 不再是当前契约；现行行为以 2026-08-18 的最终 Agent message 终态矩阵为准：全 turn 无 ID 才允许校验/repair，稳定消息之后以无 ID message 结束则直接进入可继续的 RuntimeAbnormal。prompt response 的 route watermark、200 ms quiet drain 和有界总超时仍保留。
- 2026-08-06：完成 ACP snapshot/live 水位交接的设计加固。切页恢复从固定约 400 ms、最多四次追平改为可取消的持续收敛状态机：普通完整 replay 立即静态合并并在后台等待 durable snapshot 后回收，payload 缺口则以 40 ms 起步、2 s 封顶的指数退避持续请求 `afterSeq`，缺口水位覆盖前不启用 live 打字机；卸载立即取消等待。全局路由按 64 branch 严格 LRU，不再允许 listener 绕过容量，`useSyncExternalStore` 的订阅/快照函数按 branch key 稳定；workspace 事件从“任一侧缺 projectId 即通配”收敛为 null 归一化后的严格项目身份。后端 `afterSeq` 候选先按 `newestSeq + stable range` revision 排序分页，再恢复语义展示顺序，相同 revision 原子组不被 limit 切开，避免累计旧块抬高游标后跳过后续块。Web 回归覆盖超过旧重试窗口后 watermark-only 缺口仍可追平、严格 branch 上限、空缓冲幂等 ack 与 project 隔离；Rust 回归覆盖非单调 revision 多页和同 revision 原子分页。
- 2026-08-06：收口右侧工作区与 ACP 配置选择的交互生命周期：关闭最后一个右侧资源 Tab 时同步将 `requestedOpen` 置为 `false`，普通 Dock 与紧凑 Sheet 统一收起；模型或思考强度单选通过非模态 DropdownMenu 的 `onSelect.preventDefault()` 保持菜单打开，允许连续完成模型与思考强度配置，点击外部才关闭。新增 reducer 与配置选择事件回归测试，并同步更新产品设计及 ACP UI 开发计划。
- 2026-08-06：修复 ACP `session/prompt` response 与 terminal session update 的路由收敛竞态。现有 session route frame 增加 generation 内单调 `routeSeq`，pending prompt response 捕获同 session 的 route watermark；runtime 在终态分类前按配置化 `acpPromptTerminalRouteTimeoutMs` 有界消费至水位，已消费时零等待，超时或 route 被替换时不得把 `end_turn` 认作成功。终态优先级统一为 provider/system error、cancel/interrupted、真实 end-turn success，Direct、普通 worker 与 AI-DYNAMIC leaf 共同生效；因此 provider 失败不会再被 AI-DYNAMIC 当作“成功但缺 completion artifact”进入 hidden repair。该 routeSeq 仅属于连接控制面，不替代 timeline/UI 的 `seq/headSeq`。Rust 回归覆盖延迟消费 terminal update、已消费水位零等待、水位超时不可成功、route generation 隔离与配置边界。
- 2026-08-05：修复会话切换后的 ACP 快照/实时事件断层与历史 Markdown 重播。全局事件路由新增 attempt + branch 隔离的有界 latest-wins 交接缓冲，按 64 branch、64 event/branch、256 KiB/event、512 KiB/branch、4 MiB/global 五层预算保留事件引用；超限只记录水位并通过 `afterSeq` 增量追平，非当前 branch 不触发 React 渲染。会话详情先等待全局订阅、再加载分页快照、合并后台 replay，快照覆盖稳定 generation 后回收；后端 `afterSeq` 改按语义块 `newestSeq > cursor` 返回累计 patch。Markdown 不再由整个 session active 推断历史最后一项为 streaming，只有交接完成后的当前轮 live text/thought 才动画，user/tool/terminal 边界立即完整结算。Rust/Web 回归覆盖累计 block 跨 cursor、缓冲 latest-wins/数量/字节/ack、陈旧“检查”快照首次进入即恢复完整文本与工具，以及 completed follow-up 历史静态、新 live delta 独占动画。
- 2026-08-05：完成双会话 ACP 卡死的根因修复。共享 stdout reader 从“等待单 session 4 MiB / 256 帧队列”改为非阻塞公平 demux，单 session 使用独立 64 MiB / 16,384 帧 ingress 熔断，RPC response、cancel control 和其他会话不再被旧会话反压。timeline 压缩只按 canonical rewrite 后新增 patch 字节与 patch/item 比例触发；`raw` 中超过 64 KiB 的 terminal/diff 字符串写入 `acp.file-blobs`，分页窗口和工具详情按需还原。`get_conversation_run` 首次只读摘要/会话树并移入 blocking worker，显式选中后再加载分页详情；leaf 额外投影轻量 `sessionEstablished` 和真实 ACP `sessionId`，避免 summary-first 的空 `selectedSession` 被误判为初始化中断。`stop_active_session` 先持久化暂停与 cancelled snapshot，返回 `operationId + accepted`，权限/elicitation 清理、ACP cancel、索引和详情校准后台执行，前端不再用空 session 清屏，run/sidebar 校准并行且用请求版本隔离陈旧响应。接口回归覆盖 9 MiB canonical 不重复压缩、9 MiB 已建立会话摘要不读 timeline 正文仍可恢复详情、outbound-only `session/new` 保持未建立、16 MiB 旧 session 积压下新 session 50 ms 内收帧、不可读 timeline 下停止 2 秒内 accepted、摘要/lifecycle 不读 timeline、Blob 工具详情完整还原和 accepted-stop 保留选中会话；Rust 定向测试、Web 定向测试与生产构建通过。
- 2026-08-05：模型与思考强度配置入口完成统一。工作流普通 Worker、AI-DYNAMIC 固定/动态策略和 AUTO 固定/动态配置破坏式替换旧的单模型 Select，统一复用 Direct 的 shadcn/Radix ACP 复合选择器；思考强度通过 Agent 能力目录的 `category=thought_level` 动态发现，并按真实 option id 持久化。普通 Worker/固定策略沿用节点 `config_options` / `configOptions`；动态策略新增初始分发、验收和各候选 Agent 独立 option map，runtime 按节点角色/provider 路由，避免不同 Agent 相互覆盖。切换 Agent/策略同步清理对应模型与 overrides；接口回归固定共享 override 的不可变增删、AUTO submit 规范化、动态 runtime 路由，以及工作流/AUTO 作者态全部模型槽位的选择器回显。本次按用户要求仅执行单元测试、类型检查和生产构建，不启动前端交互验证。
- 2026-08-05：修复会话侧栏 Direct Agent 图标活动态旋转环在深色主题下不可见的问题。根因为旋转环使用低对比度的 `primary` 色；侧栏及同类 ACP 运行环统一改用 `gold-running` 语义色，保留透明轨道与 900ms 动画，并增加前端回归断言固定该视觉契约。根据可见性验收反馈，侧栏环进一步调整为向外扩展 4px、外径 24px 的 2px 边框和 45% 轨道不透明度，让状态环与 Agent icon 保持明确留白。
- 2026-08-02：修复 `AskUserQuestion` 偶现停在“工具调用中”且不显示提问卡片。根因是 0.10.0 的分页保护把提问可见性同时绑定到有限事件窗口与 `timing.waitReason=elicitation`，而 elicitation 没有与 permission 对称的 session 权威字段；live timing 或 snapshot 短暂陈旧时，runtime 仍在阻塞等待但 UI 会隐藏输入入口。`AcpSessionVm` 新增从完整 timeline 投影的 `pendingElicitations`，前端 live reducer 按 request/response 更新同一字段并直接渲染，response、stop decline 与 terminal session 统一清空。Rust 接口测试覆盖分页窗口不含 request、response/terminal 收敛；Web 测试覆盖 timing 非等待态下 live request 仍可进入权威 pending 状态。
- 2026-07-25：用户消息中的隐藏 runtime context 改为由当前可见内容统一驱动气泡宽度。隐藏根节点、Trigger、Content 使用无百分比宽度的嵌套 grid stretch；`82cqi` 只保留为消息列最大测量宽度。组件在该上限内以不可见副本进行真实排版，通过 `Range.getClientRects()` 获取各文本行宽度，折叠态取标签/可见正文最大值，展开态再纳入隐藏正文；ResizeObserver、展开状态和字体加载触发重测。由此删除固定 `rem` 与线性 `65cqi` 最终宽度，避免客户端越宽、气泡尾部空白越大的问题。
- 2026-07-22：默认工作流“需求采访”开关收敛为 workspace 级偏好，仅在内置 `default` 模板显示；自定义模板拓扑不受影响。elicitation 回答后不再生成独立用户消息气泡，保留 `AskUserQuestion` 工具卡片；response signal 改由 runtime 完成 JSON-RPC 回包后清理，修复 completed run follow-up 提交后卡在“发送中”。
- 使用 Tauri 2.x + Vite + React + TypeScript 生成桌面端应用。
- `src-tauri/` 作为桌面后端，通过 path dependency 复用 Rust core 的 `App`、runtime、storage 与 config。
- `web/` 作为桌面前端，实现左侧一级功能导航 + 右侧递进式任务编排页面栈；点击“任务编排”一级入口会重置到任务列表根页面。
- 前端通过 Tauri commands 读取 task/run/round/node/artifact view model，所有终局状态仍来自 canonical state。
- MVP 实现任务列表、任务工作流、Round 详情、上下文管理和设置页；任务详情并入任务工作流页，run 详情并入工作流页 run 分组；模型管理仅作为一级导航占位。
- 工作流作者态支持对 worker 节点在 AI 输出验证与人工 check 间二选一；开启人工 check 后 ACP 节点结束时暂停等待用户在会话面板点击“成功”或“失败”，再复用既有 success / failure edge 继续执行。
- 2026-05-02：前端已按 `docs/gold-band/产品设计文档/interaction/app/原型` 对齐应用壳、任务列表 Task Preview、工作流 execution history、Round 三块工作台和设置页本地偏好控件。
- 2026-05-02：补充浏览器调试 mock view model fallback；非 Tauri 浏览器环境使用 mock 数据，Tauri 环境继续使用真实 commands，方便后续用 Vite/浏览器验证布局。
- 2026-05-03：桌面端新增 workspace 选择、最近 workspace 记忆与默认项目根解析；Tauri dev 即使从 `src-tauri/` 启动，也会向上识别包含 `.gold-band/` 的项目根。
- 2026-05-03：任务列表改为固定比例列宽，避免右侧 Task Preview 同屏时横向溢出；刷新改为保留数据的局部进度反馈，首次加载使用骨架屏；未实现动作以显式禁用按钮展示，避免含义不清的更多菜单。
- 2026-05-06：任务列表刷新反馈区分手动与后台来源：自动轮询只静默更新数据，不触发表格顶部品牌色进度条或刷新按钮高亮，避免首页运行态每秒刷新造成黄色闪烁。
- 2026-05-03：桌面端 UI 从自定义全局 CSS 一次性迁移到 Tailwind CSS v4 + `shadcn@latest`；基础控件优先使用 shadcn/ui 生成组件，Gold Band 暖金深色语义沉淀为 token，API/view model/runtime 行为保持不变。
- 2026-05-03：桌面端任务编排 IA 收敛为任务列表、任务工作流、Round 详情三页；任务详情并入工作流页 task context，run 详情并入 workflow run 分组。
- 2026-05-03：Round 详情节点选择修复为前端 camelCase 状态、Tauri command snake_case selection 入参的显式转换；运行态自动刷新改为只看结构化 run/round/node 状态，避免历史 events 文本触发持续轮询和错误条闪烁。
- 2026-05-04：工作流 execution history 的 run 分组表格改为固定比例列宽，确保多个 run 卡片之间以及 run/round 行之间列边界稳定对齐。
- 2026-05-05：修复测试问题清单中的桌面端工作流与 Round 详情问题：工作流页展示 `workflow.json.control`，任务列表和工作流历史支持分页/排序/统一横向滚动，Round 详情使用 `round.json.trace` 展示真实执行路径，并将左下区域改为 Requirement / Log / Artifact / Attachment 动态 Tabs。
- 2026-05-05：桌面端国际化改为前后端协同：前端使用 `i18next + react-i18next` 翻译可见 UI，Tauri 后端提供轻量 translator 处理后端生成的标题、summary card fallback 与缺失内容提示，同时 VM 保留稳定 key/status 供前端翻译。
- 2026-05-05：补充验收修正：工作流 control 信息移入蓝图画板，面包屑等导航标签接入 i18n，任务列表分页布局改为响应式，execution history Action 列保持可见，Round 详情小窗口改为滚动而非裁切；面包屑当前页改为短金色渐变底线，可点击上级项 hover/focus 改为文字提亮与 primary 底边线反馈，任务 ID 作为不可点击上下文标签不显示 hover 底线。
- 2026-05-06：任务编排首页视觉层级收敛，summary cards 改为中性表面 + 小面积状态强调；Task Preview 改为固定 header + 内部滚动正文，执行统计窄栏单列展示，修复底部统计贴边/超出卡片的问题。
- 2026-05-06：任务列表 Task Preview 从固定右栏改为 shadcn/ui Sheet 右侧抽屉，初始不打开；单击任务行滑出，单击其他任务行直接切换内容，单击非任务区域、Escape 或关闭按钮收回。
- 2026-05-06：Round 详情页右侧 Detail Viewer 从常驻固定列改为 shadcn/ui Sheet 详情抽屉，释放实际工作图和信息流宽度；双击节点、右键详情/会话、点击信息流条目打开抽屉，支持固定详情持续对照；固定时抽屉切换为右侧占位面板，主工作区自动收窄。
- 2026-05-06：浏览器调试模式支持轻量 deep link：`/tasks`、`/tasks/:taskId/workflow`、`/tasks/:taskId/runs/:runId/rounds/:roundId`、`/settings`，用于 agent-browser 直达页面验证。
- 2026-05-07：任务工作流页顶部 task 摘要移除“当前状态：某节点正在执行”句子；Run/Round 记录与 Round 详情的当前节点展示改为可读化格式，组合展示节点类型、workflow 节点说明和原始 node id；Round 详情实际工作图从 workflow snapshot 补齐节点说明。
- 2026-05-07：修复 Round 详情实际工作图默认视口偏下和底部裁切的问题；GraphView 改为受控 viewport，按节点 bounds 和容器尺寸计算初始平移/缩放，并移除实际工作图超过父内容区的固定最小高度，确保打开页面时执行路径图边框与节点卡片完整居中展示；浏览器 fallback 对 `/run-024/round-001` 复现两节点失败验收图用于验证。
- 2026-05-07：任务工作流页工作流默认折叠，仅保留展开入口；展开后仍显示 control 规则条与只读 GraphView，首屏优先给运行记录。
- 2026-05-08：任务工作流页将工作流入口从页面内折叠条升级为顶部“工作流”生命周期卡片，按未创建/有效/无效提供新建、查看、修复动作；完整蓝图和 control 规则条进入右侧非模态抽屉。
- 2026-07-07：会话继续/追问语义补齐：`codex-acp` 仅在同一 ACP session 首轮将 stable system prompt 作为 hidden user block 内联并持久化审计，后续停止后继续、恢复继续和完成后追问不再重复发送或记录该 system prompt；普通 worker 与 AI-DYNAMIC internal worker 的继续输入统一支持本次新附件，且不重带任务输入附件或历史附件；会话消息流中的附件预览按 timeline `raw.attachments[].path` 分流，`task-inputs/` 继续读取 task 级 `authoring/inputs`，`user-inputs/` 按 attempt locator 读取本轮新附件。
- 2026-08-18：修复追问/Runtime continue 附件“Agent 可读但应用无法回读”：`WorkerInvocation` 将 task 原始输入与本轮 user input 拆为两个显式字段，首次需求附件继续以 `task-inputs/<name>` 引用 task `authoring/inputs/`，后续图片和文本附件统一原子持久化到当前 Attempt `user-inputs/` 后再生成 ACP content block 与 timeline 元数据；Direct、固定 worker 和 AI-DYNAMIC 复用同一持久化函数，删除 Tauri 调用点静默复制与 Provider 固定写 `task-inputs` 的双路径。接口回归同时固定图片、文本、Task/Attempt 归属和持久化后的 ACP URI。
- 2026-07-07：`$new-round` 控制边新增必填 `new_round_entry`。作者态在指向 `$new-round` 的 failure 边上展示“新 Round 起点”下拉，可选 `$entry` 或真实节点；`$entry` 表示当前 workflow entry。runtime 打开新 round 后按该字段选择首个节点，不再固定从 workflow entry 重入；保存规范化只在 `$new-round` 边输出该字段。
- 2026-07-09：历史 task / run 兼容旧 `$new-round` 边：运行启动、重跑冻结 snapshot、以及运行态读取 frozen snapshot 时，若 `$new-round` 边缺失 `new_round_entry`，snapshot 专用规范化会补为 `$entry` 后再走严格校验，并只把补齐结果写入本次 `workflow.snapshot.json`；`authoring/workflow.json` 不回写，作者态新建/保存 workflow 仍保持必填校验。
- 2026-07-08：默认工作流的 `accept.failure -> $new-round` 起点从 `$entry` 调整为 `dev`，避免验收失败后重复执行方案节点；默认 workflow 节点 goal 改为按桌面语言生成中英文文案，不再硬编码英文。
- 2026-07-08：工作流控制默认分支语义调整：节点产生 `success` 或 `failure` 后若没有匹配同类型 edge，runtime 不再进入 `error-blocked`，而是等价于隐式指向 `$end`，按当前 outcome 完成 run；显式 edge 仍优先。
- 2026-08-21：工作流作者态将 failure outcome 能力收敛为 AI 输出验证或人工 check 二者共享的投影契约。开启人工 check 后画布立即展示 success / failure 双出口，允许拖线、快捷新增和边编辑；未配置 failure 边仍复用现有隐式 `failure -> $end` 语义，不增加必填校验。结果能力只进入轻量画布展示签名，不进入拓扑布局签名；结果判定模式切换不引入新状态、依赖、I/O 或整图重排。
- 2026-07-07：作者态工作流入口改为由画布拓扑自动派生：没有普通入边的真实节点显示“入口”标识；唯一入口候选自动写入 `workflow.entry`，多个或零个入口候选会阻止保存。`new_round_entry` 仅表示下一轮 Round 起点，不参与初始入口推导，也不会在拖线到 `$new-round` 时自动补默认值。
- 2026-07-09：作者态入口推导验收修正：failure 回退边指回 success 主链前序节点时不再计入初始入口入边，避免“开发 -> 测试 -> 验收，测试失败回开发”这类合法工作流被误判为没有入口；非回退的前向 failure 分支仍计入入边，防止分支节点被误识别为第二入口。
- 2026-07-07：作者态工作流画布自动整形改为使用 success 主链拓扑顺序，不再用 `workflow.nodes` 数组追加顺序判断边是否回退；后追加的前置入口节点连到原入口后会排到主链前方，failure 边继续作为分支/回退线。
- 2026-07-08：多 round 会话上下文收敛：会话树每个 round 的节点顺序优先使用 `round.json.trace.sequence`，不再受 `workflow.nodes` 数组顺序影响；普通 worker hidden runtime context 的 predecessor 默认只包含当前 round 已执行节点，只有 `$new-round` 入口节点额外包含本轮起点之前的稳定前缀节点；附件 locator 带 `round/node/attempt`，中文 hidden context 标题完成本地化。
- 2026-07-08：`$new-round` 首节点的 hidden context 新增进入本轮的触发原因：上一 round 最后触发 `$new-round` 的节点不进入 predecessor chain，但其 output artifact、预览和 attachments 会出现在入口节点的“最新前序流转原因”，用于让本轮入口节点理解为什么需要重做；本轮后续节点不继续携带该触发原因或历史稳定前缀。
- 2026-07-08：普通 worker runtime prompt 强化自由输出目录边界：attempt 根目录归 Gold Band runtime / ACP 管理，角色、任务或用户要求输出报告、脚本、过程记录等文件且未给绝对路径时，默认写入 hidden context 的 attachments 目录；hidden context 中的“附件目录”同时标注为本节点自由输出默认落点。
- 2026-07-08：默认审查 profile 明确只 review 当前开发节点 / 本轮迭代改动，优先使用 `dev-report.md` 中的文件与行号限定范围，缺失时退回当前 git diff；历史遗留问题只有被当前改动引入、放大或直接影响当前改动时才阻塞裁决。
- 2026-07-08：会话消息流新增 runtime control JSON 展示优化。普通 worker output contract 与 AI-DYNAMIC `dynamic-node-completion` 继续复用 Rust 端既有 JSON artifact 提取和校验链路；runtime 在实际消费控制输出或将非法 JSON 控制候选送入 repair 时为对应 ACP `textDelta` 写入 `raw.runtimeControlOutputDisplay`，前端只基于该标记把自然语言和控制 JSON 拆分展示，控制 JSON 收起态只显示单行 Gold Band 工作流控制条，非法候选使用告警色和告警图标，未标记 JSON 保持普通 Markdown 消息。
- 2026-08-01：会话 thought / reasoning 从 Streamdown Markdown 渲染切回 prompt-kit `ChainOfThoughtText` 纯文本展示，Markdown 标记按字面显示；展示层裁掉整段首尾空白并保留内部换行。assistant 正文继续使用现有流式 Markdown presentation。后端在独立完整 thought chunk 之间只补一个换行，避免多个思考粘连且不产生额外空白行，token 级 chunk 继续无缝累计。接口回归必须验证 `**...**`、列表符号和代码围栏不会生成 Markdown DOM，首尾换行不会撑高内容区，同时 active thought 收起时仍不占布局。
- 历史记录：桌面端品牌 Logo 曾从临时占位图替换为旧版矢量资产；该资产已由本节记录的用户提供透明 SVG 统一替换，Web、README 与 Tauri 平台图标不再保留旧品牌内容。
- 2026-05-07：修复任务编排面包屑上级项 hover/focus 高亮在页面跳转后残留的问题；可点击上级项改为纯 CSS 的 hover/focus-visible 临时反馈，Round 详情只保留当前 round 的常驻高亮。
- 2026-05-07：工作流 execution history 的 run 分组保持一致黑色背景，不使用黄色背景或左侧金线表达展开态，避免被误解为选中态；2026-05-08 起初始态所有 run 默认收起，点击整行或左侧箭头即可展开/收起。
- 2026-05-07：任务工作流页删除无效 Tabs、继续运行、停止 Run 和禁用态查看需求按钮；Workflow 与 Task Preview 的需求展示统一为单行 / 100 字截断预览，且仅在确实截断时显示完整需求入口；任务列表在当前右侧 Sheet 内切换到完整需求视图并提供返回图标。
- 2026-06-12：新会话 UI 侧边栏会话行新增删除能力；hover 操作区补齐删除按钮，删除前弹出不可撤销确认，确认后删除 `~/.gold-band` 下对应 task 目录，并在系统支持时优先移入回收站；若存在运行中的 run，则拒绝删除并提示先停止。
- 2026-05-07：统一压缩桌面端卡片 header 与内容之间的过大空白；Round 详情左下信息流、Workflow 运行记录、Workspace 最近列表、Settings 表单卡片和遗留 Task/Run 详情页均移除 Card 默认 gap、覆盖 border header 大底部 padding，并降低内容区内边距。
- 2026-05-07：Settings 页面移除标题副文案、范围提示块，以及外观/语言卡片的辅助说明文案，保留主题切换与语言选择两组本地偏好控件。
- 2026-05-07：Settings 主题选择器升级为 `Sync with OS` 开关 + 条件化主题摘要 + 抽屉式主题选择；`desktopTheme` 当前支持 `system`、`light`、`light-gray`、`dark`、`black`，浅色分为瓷白与科技灰，深色分为石墨深色与终端黑；`system` 会保留用户最近选择的浅色/深色变体；新增 `desktopFont` 偏好，浏览器调试模式优先使用 `queryLocalFonts()`，桌面端通过 Tauri `get_system_fonts` 枚举系统字体；前端验证继续通过 `/settings` deep link 使用 agent-browser 完成。
- 2026-05-08：字体选择模型从三套 CJK 预设收敛为一个内置默认字体 `app-default`（MiSans）+ 一个本机字体下拉列表；前端通过 `web/public/fonts/misans/*.woff2` 内置 `Gold Band MiSans`，默认字体预览保留彩色 sample，本机字体继续走系统枚举与浏览器 fallback 检测。
- 2026-05-08：Round 详情页移除左下“上下文”Tab，requirement 摘要上移到 Header，round 级状态保留在顶部指标区，节点详情改由工作图双击、右键菜单或详情抽屉按需查看；round 初始态不再展示单独的“编排事件”面板，Header 中“打开详情”替换为“打开日志”，节点日志由工作图右键菜单“查看日志”打开；实际工作图节点统一卡片底色，完成/失败/运行中等状态用圆形状态标记表达，产物/附件改为“产物:1”“附件:1”徽标，底部信息区只按当前选中节点的产物/附件渲染以避免切换闪烁，右键非选中节点时自动切换 selection，非固定详情抽屉用快速收起过渡后再展示菜单，日志详情长文本在抽屉内换行不撑宽容器。
- 2026-06-11：修复节点完成后 workflow 不推进的问题：ACP timeline / token 读取从 orchestrator 主控制流剥离，指标开关关闭时不读取 token 文件，开启时也只在旁路任务中读取并捕获 panic；同时修复 UTF-8 字符截断 panic、JSONL/raw log 轮转的字节切片 panic、新 UI 首个 attempt 创建前的 `Agent 调起中` 空态、attempt 创建后首个可见消息前的 `处理中...` 占位，以及 ACP 状态旋转标识的 CSS 圆环动画。
- 2026-07-08：会话式运行页将 `runtimeActive` 但 ACP session 详情暂为空的状态统一显示为 `加载中...`，避免下一节点会话 hydrate 前暴露 `conversation.runtime.runtimeActive` 内部 key 或误导为上一会话的“拉起下一节点中”；补充前端单元测试覆盖 shell 状态解析。
- 2026-08-07：会话 composer 按运行模式收口节点切换提示：Direct turn 终态后的内部 `launching-next-node` 只保留诊断事实，不再显示对客文案，Workflow/AUTO 继续显示；ACP 历史分页改为由客户端当前合并窗口与真实事件缓冲截断共同决定，移除滚动位置缓存对分页能力的反向授权，并区分完整快照与 `afterSeq/afterCursor` 增量窗口，增量响应不得用“响应之前有事件”的 `hasOlder` 覆盖客户端已完整加载的 oldest 边界，避免无历史、无用户滚动时闪现“上滑查看历史信息”。补充前端状态接口测试覆盖模式隔离、权威 false 清理、增量补帧与缓冲截断。
- 2026-05-08：任务工作流页顶部 `Latest Run` 改为统一读取最新 run，右侧 `结果` 改为复用任务列表状态 badge（如“已完成”），并移除顶部 `产物` 聚合卡片；任务列表同步移除 `资源` 列，不再在主表格展示 `Axx / Pxx`，确保首页和工作流页都只保留任务主状态与最新 run 这类高价值字段。运行记录中的 run/round 也收敛为单一状态 badge：优先显示 outcome，无 outcome 时回退到 status，不再并排展示两枚状态标签。
- 2026-05-08：任务工作流页运行记录改为固定行高摘要列表；Run/Round 主行统一使用单行截断的 current node / pauseReason 摘要，展开后直接进入 round 明细列表，不再插入重复的 run 摘要条，避免不同分页因长文本换行导致列表高度和分页器位置抖动。
- 2026-05-09：任务工作流页进一步收敛首屏主次关系：新建 Run 移入运行记录 Header，需求摘要改为无轮廓的弱强调同名单行，运行记录增加稳定列头并用中性增强表面、缩进时间线和独立 Round 行背景强化 run -> round 父子层级；随后只压缩运行记录区域自身的 Header 与行高，页面标题区保持与其他详情页统一的 Page Header 间距。
- 2026-05-09：任务列表 Task Preview 抽屉改为上方完整需求框 + 框内滚动 + 复制 icon，底部固定单一“工作流”按钮；移除抽屉执行统计、查看产物入口，并校准任务列表 Action 列表头与“进入”按钮右对齐。随后继续收敛抽屉视觉：任务列表中的完整需求区与 Workflow / Round 详情复用同一套白底单框抽屉样式，不再保留额外的彩色外框，底部工作流保持强调色主按钮；共享的完整需求抽屉组件同步补充复制 icon，并收口到标题右侧。
- 2026-05-20：任务列表默认排序从 task ID 升序调整为降序，首页优先展示最新编号任务；切回 ID 列时也保持默认降序，减少用户每次进页后手动反转排序的操作。
- 2026-05-10：任务编排三页统一为后台每 10 秒静默刷新；Workflow 与 Round 详情补充手动刷新按钮；Workflow 顶部四张 stats 卡片对齐；Round 运行状态从标题旁移动到顶部结果卡；Workflow / Round 工作图节点与 Round 顶部当前节点卡都支持“前部展示 + 尾部截断 + hover 全文”。
- 2026-05-10：Workflow 运行记录改为单展开 accordion，同一时间最多展开一个 run，降低多条 run 同时展开时的视觉噪音；工作空间选择页主视觉图标改为复用 Gold Band logo；Run 分组行操作列没有操作时不再显示横线占位。
- 2026-05-11：Workflow 运行中 Run 的操作列提供查看与停止；停止会终止当前 provider 进程树并把 run 终止为 killed；最新 Run 未终止时禁用新建 Run，避免同一任务并发启动多个 workflow。
- 2026-05-13：Workflow paused Run 仍视为可停止的非终态，运行记录操作列需要展示停止；存在当前 round 时保留查看入口，completed 等终态不展示停止。
- 2026-05-13：Round 详情工作图节点主视觉改为终态优先展示 outcome，避免 `completed + failure` 显示绿色完成；顶部指标在终态 round 中将“当前节点”改为“结束节点”。
- 2026-05-13：ACP 会话审批等待卡片收敛为信息行 + 按钮行，按钮较多时不挤压标题；等待用户权限决策时停止当前步骤计时，并将 composer 收为紧凑等待状态。
- 2026-05-11：Round 详情工作图交互破坏式升级：单击节点打开结构化详情抽屉，节点资源进入二级抽屉，会话按 `progress.events` / `raw.stream` 分离，日志从会话中独立为分页日志抽屉；默认只检索最近约 1000 条热日志，全量日志保留 30 天。
- 2026-05-12：ACP-first 重构决策：废弃新增自研 `progress.events.jsonl` 精细事件模型，后续通过 ACP 调用 agent/provider，直接使用 ACP 统一后的 session events 在 Round 节点会话详情中展示原始 agent 过程；legacy Claude Code direct / raw stream 仅作为 fallback/debug，不再驱动新的可视化协议。
- 2026-05-12：Round 详情工作图节点状态视觉收敛：未选中节点保持白底卡片，当前节点仅保留随主题联动的状态徽标，暂停态显示“已暂停”而不是运行中，只有用户明确选中节点才使用卡片级蓝色边框 / 浅蓝底 primary 强调，避免当前态被误解为选中态。
- 2026-05-12：修正任务工作流页和 Round 详情页窄客户端响应式：运行记录五列布局只在足够宽度启用，不足时改为纵向紧凑栅格；Round 工作图 Header 的选中节点说明限制在剩余宽度内截断，避免标题被挤成多行或内容向右溢出。
- 2026-05-07：任务编排首页移除页面级 summary cards 和 ModuleBar 状态 tabs，全部任务 / 运行中 / 已完成改为表格内快捷筛选，可恢复 / 失败 / 配置异常改为状态筛选，并新增任务 ID、标题、需求与最新 Run 的关键字搜索；首页定位从运行态数据看板收敛为任务工作台。
- 2026-05-07：桌面端 UI 框架层级收敛为少卡片工作台规则：AppCard 与 Metric 弱化边框和阴影，Settings 页由三张独立卡片改为单主面板 + section 分隔，主题摘要、字体选项和本地字体预览降级为低对比选项行；各主题共享同一布局层级，Tauri command、view model 和偏好保存契约不变。
- 2026-05-14：ACP 会话 agent 输出接入紧凑 Markdown 渲染，用户 prompt 保持纯文本；标题不使用文章页大字号层级，只用加粗和轻量标识表达层级；本次不引入 Pretext，后续仅在纯文本日志/Raw frame 虚拟化行高预估等测量场景再评估。
- 2026-05-15：Round 当前节点处于 `error_blocked` 时不再显示成普通已暂停，而是用错误阻塞状态和危险色展示；该状态仍暴露“继续运行”入口，ACP 最新 error diagnostic 或 Raw frame JSON-RPC error 显示为会话顶部横幅，错误后的正常 agent 输出会自动清除横幅；恢复 prompt `继续/Continue` 按独立用户气泡展示，不拼到上一条需求气泡；ACP stop 超过 15 秒未收敛时自动熔断为 `paused + process_interrupted`。
- 2026-05-17：创建任务流程升级为“创建任务 -> 导入 txt/md requirement -> 创建 workflow -> 保存任务”；任务列表移除独立导入入口，工作流编辑器基于 `@xyflow/react` 支持拖拽节点、连接边、选择 Agent、配置 JSON 输出验证和 `$new-round` 边目标，创建任务 Sheet 标题栏右侧承载“保存任务”提交入口。任务级 workflow 写入 `authoring/workflow.json`，run 启动时冻结 `workflow.snapshot.json`，Round 详情继续展示运行态快照。人工 check 仅保留 UI 占位，后端 `worker` 兼容保留但新建默认模板不再生成。
- 2026-05-18：侧边栏“知识库”升级为“上下文管理”，首版提供角色管理；用户级 profile 存储在 `~/.gold-band/context/profiles/<name>-<id>.md`。工作流节点通过分布式唯一 profile `id` 引用，编辑器使用可搜索选择器，创建/更新时间使用本地 `YYYY-MM-DD HH:MM:SS`，运行时把 profile Markdown 正文注入 prompt bundle。
- 2026-07-09：工作流已支持多 workspace，profile 自定义角色收敛为用户级唯一来源；运行时不再读取项目级 profile，前端上下文管理和工作流 profile 下拉不再展示项目级概念，不提供迁移或双读兼容。
- 2026-05-18：默认角色扩展为方案、开发、审查、测试、验收、清理六类；默认 workflow 初始化时先同步默认角色，再将可用 profile `id` 绑定到 `plan/dev/review/test/accept/cleanup` 节点。默认路径更新为 `plan -> dev -> review -> test -> accept -> cleanup -> $end`，accept failure 新建 Round 后从 `dev` 重开，cleanup 为普通 worker 节点且不启用 AI 输出验证；保存 workflow 时集中校验必填字段、角色绑定和角色存在性，错误弹窗关闭后在字段处红色标注。
- 2026-05-20：修复 ACP JSON-RPC 帧判定：adapter 发起的 `session/request_permission` 即使与当前 `session/prompt` request id 相同，也按 inbound request 处理，不再误判节点已完成并提前进入 artifact 归一化。
- 2026-05-20：收敛 provider system prompt：未声明 `output` 的节点会被明确告知无需产出 canonical artifact 或查找 artifact/output 约束；当前节点上下文由 prompt 给出，前序产出仅按 prompt 明确给出的路径读取，`run_dir` 只作为这些路径的父级上下文，避免节点为寻找未声明产物或确认约束主动扫描 run 目录。前序节点结果统一进入 system prompt 的执行链、artifact 路径和 preview，不再以 `Current Feedback` 注入 user prompt；跨 round 链路用 `-$new-round->` 说明新轮次来源。
- 2026-05-21：ACP session 累计处理耗时改为净耗时，扣除所有 `session/request_permission` pending 到用户选择之间的阻塞式用户等待。
- 2026-05-21：ACP 会话详情新增“系统提示”入口，从 raw frame 中解析 `session/new._meta.systemPrompt.append` 并用弹窗只读展示实际追加的 system prompt。
- 2026-07-28：ACP 系统提示弹窗默认使用现有 prompt-kit Markdown 渲染器展示，保留“渲染 Markdown / 原文”切换并通过独立本地偏好记忆，不与产物预览模式或 ACP session 数据耦合；多 attempt 切换继续沿用当前查看模式。
- 2026-07-28：ACP 系统提示弹窗在桌面断点显式覆盖 shadcn Dialog 默认 `sm:max-w-lg` 为 `sm:max-w-5xl`，解决调用侧普通 `max-w-*` 无法覆盖响应式默认值导致的窄弹窗问题；小窗口继续保留组件默认视口安全边距。
- 2026-05-23：continue 恢复路径改为重新渲染当前节点 system prompt，并随 `session/load._meta.systemPrompt.append` 传给 Claude Agent ACP；ACP 内部 create session 表示用 SDK `resume` 创建 query 进程，不改变 Gold Band 的 continue 语义。系统提示入口应同时解析 `session/new` 与 `session/load` 的追加内容。
- 2026-05-23：Codex ACP 0.14.0 会忽略 ACP `_meta.systemPrompt`；Gold Band 对 `codex-acp` 在 `session/prompt` 前内联当前节点 system prompt，避免首次调用丢失节点约束。
- 2026-08-06：ACP 自动重试收敛为稳定 prompt turn；orchestrator 在调用 provider 前一次性分配 `promptId`，并在每次自动重试的 runtime 重建中传回 `PromptBundle`，不再依赖短生命周期 ACP runtime 或 timeline 扫描恢复身份；`acp.snapshot.json.promptRetry` 持久化逻辑 turn 的 retry counter 与 canonical timeline event identity，即使 session 已 terminal 也保留，下一 logical turn 才覆盖。普通 worker、Direct 与 AI-DYNAMIC leaf 共同使用 retry policy，后者只重试当前失败 leaf。retry 携带进度，runtime 重建继续 upsert 同一个 processing 用户事件，UI 只渲染单一气泡；进行中的“正在重试 x/n 次”复用低强度 opacity 呼吸动画，并遵守 reduced-motion，成功后清除 footer，失败/停止终态文字保持静态。用户停止时当前消息结算为 `cancelled` 并固定保留已发生的 retry 次数，耗尽预算才结算失败。durable cancellation 已提升到 attempt 级 `cancel_attempt_prompt()`：停止入口在 timeline 文件锁内用单调 revision 原子 upsert 最新 processing retry prompt，因此 retry backoff、runtime initialize/session setup 与 active RPC 三个阶段都不再依赖 runtime-local `activePromptTurn`；重复停止幂等，普通 processing prompt与历史终态 prompt 不被改写。runtime initialize/session setup 仍从已加载 timeline 恢复 processing retry 事件以保持内存一致。visible prompt 与 hidden repair 按 `promptId + visibility` 隔离，防止内部修复覆盖可见消息。每次真实 ACP prompt RPC 继续使用独立 usage transaction ID，所有返回有效 usage 的尝试均计入 attempt totals，与 canonical timeline 去重互不影响。前端 session/live reducer 按 event lifecycle seq 单调合并，旧 processing/completed snapshot 不得覆盖较新的 cancelled/failed 终态；按 `promptId` 折叠多物理事件只保留为旧历史兼容。终态结算由本次逻辑 prompt 持有的完整用户事件写回相同 timeline ID，不再从只保留运行中工具/交互的 hot cache 查询，因此已完成 retry 消息也能正确落为 cancelled 或 failed。`systemError` 即使伴随 `end_turn` 也落为标准 error 诊断和 failed turn，确保错误横幅、会话详情和侧栏终态一致。
- 2026-05-23：桌面 ACP 会话面板的手动追问入口改为复用当前节点 prompt bundle，`session/load` 恢复旧会话后也重新追加节点 system prompt，避免用户追问时模型忘记输出 DSL。
- 2026-08-07：ACP detached session 恢复改为 capability-driven 策略：普通上下文续接优先使用稳定的 `session/resume`，仅在 resume 未声明时回退到 `session/load`；开启跨端完整历史同步时继续要求 load。恢复状态机拆分为无回放恢复与历史回放，resume 不启动 importer/quiet-drain，load 保留延迟 replay 防护；两条路径都在 prompt 前统一重放模型、原生权限模式和 config options。缺少能力时返回 `acp.session-restore-unsupported` / `acp.history-sync-unsupported` 结构化错误；Raw Frame、system prompt 提取与 stale close fuse 全部识别 `session/resume`。
- 2026-05-24：`max_attempts` 收敛为 round 内修复/重试预算，只统计 `failure` 修复跳转；超限时写入结构化控制失败原因。Round 详情工作图按逻辑节点合并多 attempt，以 attempt 标记和 ACP conversation 聚合展示 continue/new 会话差异；`session=new` 始终独立成可切换 conversation，只有后续 `session=continue` 才挂回被继续的 conversation；运行中 synthetic/provider echo 的同文 user prompt 只展示一条。
- 2026-06-04：AI-DYNAMIC 节点补齐与普通 worker 一致的权限模式配置，并将该权限作为 bootstrap / 派生 worker / merge / acceptance 的统一默认权限继承；权限字段最终以 provider doctor 返回的真实 ACP mode id 为准，产品侧虚拟权限名只作为可解析输入之一。Round 详情主图不再把 AI-DYNAMIC 仅视为一个复合占位节点，而是直接内联其实际执行的动态节点，并通过 outer locator 复用普通节点的详情、会话、Raw frame、artifact 与 attachment 查看链路。
- 2026-05-21：工作流编辑器的节点 id 输入改为本地草稿提交，避免中文输入法 composition 阶段被受控值和 sanitize 打断；作者态画布普通节点直接展示原始 id，不再把 `test` 等默认模板名称本地化显示。
- 2026-05-21：AI 输出验证的 JSON 输出约束输入改为本地草稿 + 延迟校验，停止输入约 2 秒或失焦后再写入 DSL；自动 beautify 改为输入框右上角手动美化按钮，避免编辑半截 JSON 时被重排。
- 2026-08-11：Release Please 明确启用 `bump-minor-pre-major`。在正式进入 `1.0.0` 前，带 `!` 或 `BREAKING CHANGE` 的提交从当前 `0.x` 版本提升 minor 并归零 patch，例如 `0.12.4` 发布为 `0.13.0`；进入稳定版后的 major 版本规则不受影响。配置契约由 `npm run test:release-config` 固化，避免发布策略被后续配置调整意外移除。
- 2026-05-25：桌面端接入 Tauri updater，按 `default` / `wb` 构建渠道隔离更新配置和 public key。default 渠道指向 `https://github.com/diodeme/Gold-Band/releases/latest/download/latest.json`，`release-please` 在创建 draft release 后会先确保对应 git tag 指向 release commit，再于同一 workflow 构建 default 桌面安装包、签名并上传 `latest.json`；该 workflow 支持 `main` push 自动触发和 GitHub Actions 页面手动触发，手动触发用于补跑 release-please 主链路；updater manifest 生成时显式使用 release tag，避免 workflow_dispatch 分支名进入 `version` 或下载 URL；Windows 平台优先选择签名的 setup exe 作为更新安装包；macOS arm64 使用 `macos-15`，macOS x64 使用 `macos-15-intel`；publish 后客户端才通过 latest 地址看到更新。独立 `Release` workflow 仅作为手动输入 tag 的重建 fallback，重建时应用源码来自 release tag，发布脚本和 manifest 生成逻辑来自所选 workflow 分支。wb 渠道使用内网占位地址，本地 `npm run build:wb` 打包后由人工上传内网包与 JSON；本地生成 `latest.json` 时必须优先匹配本次构建 version 对应的签名安装包，避免目录残留旧包时 URL 指回历史 exe。
- 2026-05-25：设置页改为 `通用 / 外观 / 高级` tabs，高级页支持保存用户级 `desktopUpdaterUrlOverride`、恢复内置地址、手动检查更新和展示后台检查状态；用户覆盖 URL 不改变渠道 public key，避免 default / wb 串包；`desktopUpdaterLastCheckedAt` 持久化最近一次检查时间，展示为本地系统时区 `YYYY-MM-DD HH:MM:SS`。
- 2026-08-21：修复 `wb` 静默关键更新轮询的重复 I/O。后台单轮检查保留 Tauri `Update` 作为该轮唯一结果，同时投影 UI 状态并判断 `critical`，不再由静默下载路径第二次请求 manifest；同一版本已有完整 pending 文件时跳过安装包下载，下载完成后通过既有原子写入能力提交最终文件，再登记 pending 状态。接口回归以本地 HTTP updater 固定静默渠道单轮仅一次 manifest GET，并覆盖相同/不同/缺失 pending 文件与完整落盘；`default` 渠道的静默更新配置保持关闭。
- 2026-06-12：高级设置中“记录详细日志”“开启指标上报”的常驻说明文案改为 tips icon tooltip 形式，减少长说明占位；“开启指标上报”标题颜色与相邻设置项统一为 muted heading 样式；这两项开关统一放到标题行内而不是远端右对齐。
- 2026-08-16：高级设置移除“使用本地 Claude”开关和页面挂载时的本地可执行文件探测；前端保存设置时固定提交 `useLocalClaude = false`，后端 `RuntimeConfig` 加载入口也固定投影为 `false`，历史持久化的 `true` 不再影响运行时，旧用户升级后立即使用 ACP npm 包内版本。后端字段、接口和既有 ACP 解析代码继续保留，未来重新开放时恢复单一配置投影与前端入口即可。
- 2026-05-27：更新提示新增分层红点：后台发现当前可更新版本时，左侧 Settings、设置页 Advanced tab 和 Updates 分组标题同时提醒；Settings 和 Advanced 的已读状态按版本号持久化，用户逐层进入时只清当前层，Updates 红点仅在当前无可更新版本时消失。
- 2026-05-27：右侧主内容区顶部新增一次性更新公告区；首次发现某个新版本时展示公告，点击后弹窗引导用户前往 设置 → 高级 → 更新；公告关闭状态与可用更新快照一并持久化，重启应用后若版本仍可更新则公告继续可见，直到用户关闭或后续检查确认无更新。
- 2026-05-27：修正更新状态区的缓存展示语义；当重启后仅命中持久化的可用更新快照、实时 `updateStatus` 仍是 `idle` 时，UI 仍按“可更新”态展示状态文案、版本号和安装入口，避免出现“尚未检查”与可更新版本并存。
- 2026-05-26：Windows release 桌面包使用 GUI subsystem，安装后双击启动不再附带 cmd 窗口；debug/dev 构建仍保留控制台输出。后台子进程统一通过 process helper 设置隐藏窗口，Windows 进程树清理丢弃 `taskkill` stdout/stderr，ACP provider 的 npx/codex 等子进程同样不弹控制台窗口。
- 2026-06-04：桌面端左右侧 Sheet 抽屉统一支持边缘拖拽调宽与本地宽度记忆；`SheetContent` 负责默认调宽能力、视口边界约束和 localStorage 持久化，各页面只补稳定 `resizeStorageKey` 与宽度上下限；修正首次打开任务预览时拖拽手柄抢占焦点导致的蓝色高亮，要求手柄默认隐藏、悬停弱提示、拖拽中再高亮。
- 2026-06-11：会话式运行页 compact composer 用量栏恢复具体处理状态标签，运行中必须展示“思考中...”/“工具调用中...”等当前步骤文案；后端工作流在节点完成后立即持久化下一节点或新 round 的 `run.current* / round.trace / node.json`，并隔离 metrics 回调 panic，避免出现当前节点已 completed 但工作流长期停在 running 旧节点的状态裂缝。
- 2026-06-11：修复新 UI ACP 会话的跨节点自动跳转策略；前端把“是否允许自动跟随 running session”提升为显式状态，只有当前消息窗口贴底且用户仍在跟随当前运行会话时，新的 ACP live event 才会把选中会话切到下一运行节点。用户手动切到其他 session 或滚离底部后，后台节点继续运行，但不会再抢占当前会话视图；run VM 刷新若未命中自动跟随条件，必须保留既有 `selectedSessionKey`，并且手动切换与已排队的 live refresh 冲突时，手动选择优先。
- 2026-06-12：会话页手动切换后的 auto-follow 判定改为基于 `run.activeSessions` 是否包含当前选中 session，而不是依赖叶子节点自身的 `runtimeDisplay.tone`；这样已完成节点在树状态短暂滞后时，也不会被误判为仍应跟随并再次跳回后台运行节点。
- 2026-06-12：修复新 UI 默认选错 session 的问题。run VM 无显式 `selectedSessionKey` 时默认按 attempt 开始时间选择最新 session，避免 task-040 这类最新 `开发/attempt-002` 被 workflow 顺序最后的 `测试/attempt-001` 抢占；`process-interrupted` 可继续态仍保留 composer 输入触发 workflow runtime continue 的既有设计。
- 2026-08-17：补齐 auto-follow 跨页面重挂载的 run 级生命周期。Conversation run 的 12 项内存 LRU 现在原子保存 `followMode + selectedSessionKey`；用户查看历史 attempt 或滚离底部后，切换到其他页面再返回会恢复原 attempt，并继续命中既有 ACP event-window 滚动锚点。删除 `conversationPage` 变化与 `ConversationRunPage` mount 时无条件恢复 auto 的入口；侧边栏“快速对话”、工作区“新会话”、搜索结果和通知跳转也统一经过 cache-aware 导航边界；ACP viewport 首帧直接使用缓存的 `atBottom` follow 意图，避免子组件先以贴底状态覆盖滚动锚点；initial-load 使用 remembered session key 请求正文并在后台快照合并时保留 manual selection，显式 attempt deep link 仍拥有最高优先级。新增缓存、reentry 选择、导航入口、滚动初始状态和组件重挂载回归测试；不增加后端字段、持久化 I/O、无界缓存或新的滚动实现。
- 2026-06-12：补齐会话页运行中停止链路并收敛为统一入口。新 UI composer 不再在前端区分普通 ACP prompt 与 workflow runtime continue，而是统一调用桌面 `stop_active_session`；后端内部判定 run running 时复用既有 `App::run_pause` 完成 run 暂停、当前 attempt cancel、provider pid 清理和 dynamic descendants 暂停，run 已非 running 但 ACP 追问仍活跃时复用 `cancel_acp_session` 停止该 ACP session，避免前端和 Tauri command 层复制第二套停止逻辑。
- 2026-06-25：runtime 增加 `runtime-abnormal` 可继续异常暂停，用于本地 IO/资源、ACP transport 或 driver 异常，区别于 provider/model/workflow 前提错误导致的 `error-blocked`；JSONL append/roll/timeline overwrite 按同一路径串行化，避免并发写坏一行 JSONL；AI-DYNAMIC continue 前会先接受已完整落盘的 `dynamic-node-completion`，避免 session 已完成但 driver 异常暂停后重复发送；doctor ACP 目录改为临时/有界诊断产物，成功后删除、失败时只保留最近一次 bounded bundle。
- 2026-06-28：修复关闭应用/启动恢复后权限申请重复弹窗的问题。停止流程中的 attempt cancel 现在会同步把未决 ACP permission request 写成 `cancelled` response，并 upsert `acp.timeline.jsonl` / legacy `acp.events.jsonl` 的 `permissionRequest(status=cancelled)`；ACP prompt 的 cancelled/interrupted/error 收尾路径也会执行同一 pending interaction 收敛。`AcpSessionVm.events` 即使做分页裁剪也会附带每个 permission request 的最新终态，用来覆盖前端缓存中的旧 pending。重进页面只回放取消/已选择事实，不再恢复权限弹窗；迟到的旧弹窗授权不能把已取消权限反写为 `selected`。已选择的 `selected` 权限事件不会被停止流程覆盖。前端 ACP event 合并改为按 canonical permission request id 替换 permission 事件，不再把 `sessionId` 混入权限请求身份；后端 cancelled permission event 继承原 pending event 的 session/tool/raw 上下文，避免同一权限裂变为旧 pending 与新 cancelled 两条 UI 事实。
- 2026-06-29：ACP permission / elicitation 的 request-response JSON 文件收敛为临时信号文件，长期事实源统一为 timeline/events。runtime 消费响应并写入终态事件后会清理对应 request/response 文件；非 active session 的 command-side durable replay 写完终态事件后也会清理信号文件。停止流程写出的 cancelled response 保留到 live waiter 消费，避免提前删除导致阻塞线程无法解除。
- 2026-06-29：AI-DYNAMIC driver 热循环持久化改为按 `DynamicGraphState` 内容指纹去重；graph 未变化的 200ms worker 等待轮次不再重复重写 graph/run/node/group/proposal JSON，ready/launch scheduler 诊断事件也只在实际 promoted ready 或实际 launch 时写入，避免无意义磁盘 I/O 和 JSONL 心跳膨胀。
- 2026-06-29：ACP elicitation 卡片视觉密度收敛：已确认回答、多步骤进度、题干、选项行、自定义输入与底部操作区统一压缩上下留白和控制高度，保持会话流内联提问的轻量表单形态，不改变 request/response 协议与答案提交语义。
- 2026-06-29：前端构建类型检查拆分为生产源码配置 `web/tsconfig.build.json` 与 Vitest 测试运行配置；`npm run web:build` 不再把 Node 环境测试文件纳入浏览器源码编译，测试验收继续通过 `npm run web:test` 固化。
- 2026-06-29：wb 构建链路补齐 MCP stdio 握手实现对 `std::process::Command` 的显式依赖，保持新增 stdio MCP health/tools 探测逻辑可被 Rust 编译器稳定解析。
- 启动：`npm run dev`；默认渠道固定快照调试：`npm run dev:static`（前端构建直接写入本次进程独占的不可变快照，Tauri 只服务该快照并在退出后清理；其他 `web:build` 不再触发全局刷新或深层路由临时 404。该模式同时关闭 Vite HMR、Tauri source watcher 与 Rust debug symbols，并使用独立 Cargo target，源码修改不影响当前客户端且规避 Windows PDB 冲突/容量限制；普通 dev 调试能力不受影响）；构建：`npm run build` / `npm run build:default`；wb 本地构建：`npm run build:wb`。
- 仓库级依赖安装与锁文件统一使用 `npm` / `package-lock.json`；除非单独立项迁移包管理器，否则不新增 `pnpm-lock.yaml`、`yarn.lock` 等并行 lockfile。

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
  worker/
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
- `节点输出产物`
- `节点输出产物`
- `验收输出产物`

职责：
- schema struct
- parse / validate
- write canonical json
- 从 provider result 提取并校验 output artifact

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

### 8. `worker/`
负责执行 `节点输出产物`。

包括：
- 读取当前 round 最新 `节点输出产物`
- 串行执行 commands
- fail-fast
- 生成 `节点输出产物.json`
- 写 `stdout.log` / `stderr.log`

这一层不混 control 逻辑，只返回 worker 结果。

### 9. `control/`
MVP 核心。

负责：
- 根据 node result 归纳 outcome
- 查 edge
- 判断 `$end`
- 判断 `failure 边`
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
应用服务层，串起 CLI、runtime、provider、worker、control。

例如：
- `start_run()`
- `continue_run()`
- `retry_run()`
- `pause_run()`
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

桌面端 `start_run` command 需要在第 4 步完成后立即返回初始 run summary，并把第 5 步交给后台线程执行，避免 UI 等待完整 workflow 跑完后才恢复响应。若最新 Run 尚未进入终止态，桌面端不允许继续新建 Run。

run 创建编号规则统一由 runtime 负责：普通启动和会话页重跑都扫描当前 task 的 `runs/` 目录最大 `run-NNN` 后递增，并先原子创建目标 run 目录占位，再写入 `run.json`、`workflow.snapshot.json`、`round.json` 和首个 `node.json`。前端不得根据当前选中的 run 推导新 run id；并发重跑时目录占位失败的一方必须重新扫描最大编号再分配。

### `run kill`
MVP 行为：

1. 读取当前 run / round / node / attempt
2. 若当前 attempt 存在 provider 进程记录，则终止 provider 进程树
3. 将 run、当前 round、当前 node 写为 `completed + killed`
4. 后台 workflow 驱动在发现 run 已 killed 后停止推进，不再把 run 覆写回 paused 或 running

### 如果 node 是 `worker`
- resolve provider/profile
- 生成 invocation
- `goal -> taskInstruction`
- 调 provider
- 生成 artifact / worker-ref / node.json
- control 决定下一步

### 如果 node 是 `worker`
- 读取当前 round 最新 `节点输出产物`
- 执行 commands
- 写 `节点输出产物`
- control 决定下一步

### 如果 node 是 `worker`
- 组装默认 evidence package
- 调 provider
- 写 `验收输出产物`
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

### `worker`
- `success`
- `failure`
- `invalid`

### `worker`
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

### schema 输出修复规则
- 声明 `output.schema` 的 worker 输出不合法时，不走 edge。
- 普通 workflow / AI-DYNAMIC 工作节点先完成自然语言业务 turn，再在同一 attempt / provider session 中通过隐藏 finalize turn 请求 canonical artifact；只有 AI-DYNAMIC bootstrap 分发控制节点在首轮内联输出协议。
- runtime 使用 attempt 根目录的 `artifact-emission.json(finalizing)` 固化两阶段边界；恢复或重试检测到该状态时只继续 finalize，不重复业务 turn。
- runtime 在同一 artifact finalize 会话中隐藏追问 agent 修复输出。
- 隐藏追问最多 3 次；仍不合法则 workflow failure。

---

## MVP 文件落盘

### worker attempt
```text
attempt-001/
  node.json
  worker-ref.json
  artifacts/
    节点输出产物.json   # 如果有
  attachments/
```

### worker attempt
```text
attempt-001/
  node.json
  节点输出产物.source.json
  artifacts/
    节点输出产物.json
  commands/
    01-build/
      command.json
      stdout.log
      stderr.log
```

### output validation attempt
```text
attempt-001/
  node.json
  worker-ref.json
  artifacts/
    验收输出产物.json
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

### 2026-06-06：需求标题归一化实验工具
- 新增独立可运行的 Rust bin：`src/bin/requirement_title.rs`
- 目标：接收 requirement 文本文件路径，输出一个约 10 字左右的中文短标题，优先服务 txt / md / 纯文本导入场景
- 当前实现策略：采用结构优先 + 自然语言回退 + 轻量统计压缩的三层管线，不依赖大模型
- 具体顺序：先尝试抽取 H1/主标题等强结构信号；若输入缺少结构，则回退到前导主题句；若仍过长，再依据重复度、位置和技术实体显著性压缩标题
- 当前范围：先只支持中文，作为后续多语言 `language profile` 架构的最小切片
- 当前仓库主 lib 另有独立编译问题时，可单独用 `rustc --edition=2024 src/bin/requirement_title.rs -o .claude/requirement_title_standalone.exe` 验证该文件逻辑
- 常规验证方式：`cargo run --bin requirement_title -- <文件路径>`

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

### Phase 3：接通 worker / output validation
9. worker runner
10. 节点输出产物 writer
11. output validation invocation
12. 验收输出产物 writer

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

### 测试目标

将本节作为 MVP 的主测试计划入口，用于验证 `worker-only 工作流` 主链路、repair loop、acceptance loop 与异常恢复机制是否形成可重复执行的闭环。

### 测试范围

- task / workflow 读取与运行初始化。
- `worker` 节点执行与 artifact 落盘。
- `节点输出产物` 产出后的 `worker` 执行。
- `worker` 执行与 run 最终状态收敛。
- `continue` / `retry` / `open-session` 等恢复入口。
- run 状态、artifact、session 等 CLI 检查能力。

### 不在本次范围

- 不验证超出 MVP 边界的高级调度、并发编排或额外 provider 扩展。
- 不用单一 happy path 代替异常恢复验证。
- 不用只看日志输出代替 run 状态、artifact 和会话状态检查。

### 测试前置条件

- 准备可运行的最小 task / workflow 示例。
- provider、运行命令与必要环境变量已就绪。
- runtime layout 可正常创建 run、round、artifact 和状态文件。
- 测试执行者可使用 `run start`、`run status`、`continue`、`retry`、`open-session` 等入口。

### 核心测试场景

#### 场景 1：`worker-only 工作流 -> success`

- 前置条件：`worker` 能生成合法 `节点输出产物`，`worker` 与 `worker` 均可成功执行。
- 操作步骤：启动 run，等待 `worker`、`worker`、`worker` 依次完成。
- 预期结果：run 最终状态为 `completed + success`。
- 关键产物或状态：worker artifact、worker 结果、output validation 结果、最终 run 状态均已落盘且可查看。
- 失败判定：任一阶段未产出预期文件、状态未收敛或最终状态不是 `completed + success`。

#### 场景 2：`worker failure -> repair -> worker success -> output validation success`

- 前置条件：首次 `worker` 会失败，系统允许进入 repair loop。
- 操作步骤：启动 run，触发 `worker` 失败，执行修复后重新运行 `worker`，再进入 `worker`。
- 预期结果：repair loop 生效，后续 `worker` 与 `worker` 成功，run 最终成功结束。
- 关键产物或状态：失败原因、修复后的新输入、重试记录与最终成功结果均可追踪。
- 失败判定：`worker` 失败后无法进入修复流程，或修复后状态、产物、轮次记录不一致。

#### 场景 3：`output validation failure -> auto_loop -> new round -> success`

- 前置条件：首次 `worker` 返回失败，系统允许进入 acceptance loop。
- 操作步骤：启动 run，执行到 `worker` 失败，触发自动 loop，进入新 round 后再次完成主链路。
- 预期结果：acceptance loop 生效，新 round 可以继续推进，最终收敛为成功状态。
- 关键产物或状态：output validation 失败原因、新 round 状态迁移、后续 round 产物与最终结果均清晰可追踪。
- 失败判定：`worker` 失败后未生成新的可执行 round，或 loop 行为与文档定义不一致。

#### 场景 4：`worker invalid / interrupted`

- 前置条件：`worker` 返回非法结果，或执行过程中被中断。
- 操作步骤：启动 run，触发 `worker` 非法输出或中断，再执行 `run continue` / `run retry`。
- 预期结果：恢复入口行为符合文档，能够区分继续执行与重新尝试的边界。
- 关键产物或状态：中断前状态、恢复后的 run / round 状态、重试结果与会话入口均可检查。
- 失败判定：恢复命令语义不清、状态被覆盖、产物丢失，或无法继续排查原因。

### 验收通过标准

- 上述 4 个场景全部至少成功验证一次。
- 每个场景都能同时验证状态流转、artifact 落盘与 CLI 可观测性。
- 异常场景必须能定位失败阶段，并能通过文档定义的恢复入口继续处理。
- 不允许出现 run 最终状态与实际产物不一致的情况。

### 结果记录方式

- 记录每个场景的输入、执行步骤、最终状态与关键产物路径。
- 记录失败场景的触发方式、恢复动作与最终结论。
- 回归时至少重复执行上述 4 个核心场景。

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
- `worker`
- `节点输出产物`
- repair loop

### Slice 3
- `worker`
- acceptance loop
- `$end`

### Slice 4
- `continue / retry / open-session`

---

## 2026-07-21：工作流长运行内存稳定性隐性优化

- 生命周期：桌面进程共享一个 `RuntimeLifecycleBus`，metrics、notifications、conversation-run-state 在 setup 以固定键幂等订阅一次；保留匿名订阅供测试和内部场景使用。
- ACP 传输：每 session event pump 使用 4 MiB / 256 帧 FIFO 消费窗口；2026-08-05 起共享 stdout reader 不再等待该窗口，改由 64 MiB / 16,384 帧 session ingress 隔离过载，避免一个 session 反压 RPC 与其他会话。
- Timeline：磁盘 `acp.timeline.jsonl` 是 canonical base + patch journal 规范索引，大字段使用 attempt Blob；内存只保留当前 text/thought/plan 流、未终态 tool、未决 permission/elicitation 及 metadata/usage/timing。会话树只加载 metadata/lifecycle，显式选择会话后才加载分页事件详情。
- 日志：未路由 frame 仅记录摘要并按连接/事件类型限频；`runtime.log` 8 MiB 轮转、保留 4 份并继续执行 30 天清理，`acp.raw.jsonl` 保持现状。
- 2026-08-20：修复 Windows ACP adapter stderr 被强制按 UTF-8 逐行读取、首次解码失败即停止消费的问题。stderr 改为 4 KiB buffer 流式读取与 16 KiB 单行有界保留，非法 UTF-8 使用 lossy 文本继续消费，并在详细日志附带编码、大小、截断状态及最多 256 字节十六进制前缀。stdout/stderr/进程退出日志统一携带 canonical provider id、adapter identity 与实际 command，异常 stdout EOF 补记可获得的 exit code/status，避免多个 npx Agent 无法归因及真实 npm 错误被 `ACP adapter transport interrupted` 覆盖。接口测试固化非法字节后的后续行仍可读、超长无换行输出内存有界；未新增依赖、缓存、队列或持久状态，读取整体 O(n) 且不进入 ACP stdout/消息处理热路径。
- 2026-08-21：完成桌面 `runtime.log` 会话审计与分级收敛。创建、prompt admission/queue、continue、stop、terminal 等用户低频边界使用结构化 `INFO`，同步/后台失败及自动队列、MCP 降级使用 `WARN`；每 60 秒 doctor/命令目录周期、ACP raw/RPC、adapter stderr 正文及非 UTF-8 摘要统一为“记录详细日志”控制的 `DEBUG`。日志只携带 canonical locator、turn/operation/revision/outcome，不记录 prompt、附件路径、工具内容、Token 或原始 frame。`runtime.log`、`acp.raw.jsonl`、`acp.diagnostics.jsonl` 明确为 best-effort 旁路，写入失败不得覆盖 provider 原错误或改变 RPC、取消、队列与 terminal；canonical timeline/session/worker/run 状态仍保持强一致写入。补充不可写 sidecar、非 UTF-8 连续消费、有界长行与 prompt 队列生命周期回归测试，并同步修正桌面测试夹具的 ACP storage schema version；5 项日志定向测试、1 项桌面队列接口测试及 `cargo check --workspace` 均通过。未新增依赖、状态、缓存或重试，单个用户动作只增加 O(1) 结构化事件，周期日志默认关闭。
- 兼容边界：Tauri command、Runtime API、ViewModel JSON、前端类型、既有事件窗口配置、75ms/125ms 流式刷新、消息/工具/权限/分页/自动跟随与 workflow 并行度全部不变；不包含 WebView 恢复和高内存降并行。
- 回归固化：覆盖具名订阅幂等、10,000 帧 FIFO、session ingress 过载隔离、超大帧与关闭、热状态释放后 timeline 可读、tool input/output/Blob 合并、permission/elicitation timing、非选中不可读 timeline、日志限频和 size rotation。合入前必须通过 Rust workspace、Web test/build 与桌面 deep-link 验证。
- 本次结果：Rust workspace 全量通过；Release ACP route 10 项通过；Web 54 个测试文件、362 项通过且生产构建成功；桌面端现有 run/session deep-link 冒烟通过，测试实例与 dev server 已清理。字体测试仅修正元素定位，未改变 UI。

---

## 2026-07-22：ACP 流式 Markdown 顺滑呈现

- 根因修复：不再把 75ms/125ms 的 canonical snapshot 合并节奏直接当成视觉输出帧率，也不再把完整 snapshot 提前放入 DOM 后用透明字符模拟逐字。唯一活跃 text/thought item 使用局部 presentation controller 稳定推进可见 offset，消息框只按真实可见前缀增长，消除底部大块预留空白和跨 block 零散字符。
- Markdown：prompt-kit Markdown copy-in 使用模块化 `streamdown` 核心，流式阶段对当前可见前缀做不完整语法修复；完成后停止 presentation/incomplete repair，但已流式组件保持同一 block renderer DOM，重新加载的历史消息才直接 static。syntax guard 吞并纯 Markdown 控制符和未完成链接地址。Streamdown 不再启用全字符 opacity/stagger，避免 block 更新重播历史字符。思考过程与普通 assistant 消息统一实时 Markdown。
- Thought canonical：Claude Code 独立 thought chunk 原始数据不带换行，后端 accumulator 对完整 strong block chunk 写入段落分隔，token 级 chunk 保持连续；前端只展示 timeline canonical，不对旧会话增加内容修复或兼容重写。
- Thought 折叠生命周期：active streaming thought 收起时通过 Radix `forceMount + hidden` 保留 Markdown presentation 实例与 visible offset，再展开不重放；完成后的历史 thought 恢复普通按需挂载，控制常驻开销。
- 活跃生命周期：streaming item 由完成 snapshot/live 水位交接后的事件来源显式驱动，不再按整个 active session 的历史最大 `endedSeq/seq` 推断。快照与 replay 静态显示；当前 prompt 之后新到达的 live text/thought 才成为唯一 streaming item，tool、plan、permission、user 或 terminal 边界立即结算。保留 timeline 稳定 id、工具/权限即时路径、分页和自动跟随。
- 性能边界：只有当前活跃尾部运行约 32ms 的呈现帧，历史消息无 timer；速率根据 backlog 在统一变量范围内自适应，单帧 elapsed 有上限，避免标签页恢复或大批次导致瞬间跳跃。不启用 Shiki、Mermaid、KaTeX 插件。
- 回归要求：必须通过 presentation/Markdown/活跃流接口单测、全量 Web test、生产构建，并在前端 deep link 中验证 thought 与普通消息的实时粗体、列表、代码围栏、容器无预布局空白、批次积压平滑追赶及 terminal 最终收敛。

---

## 结论

建议主实现语言使用 Rust，先围绕 CLI + runtime + Claude Code provider 跑通 MVP 闭环，再逐步补 provider 扩展、progress 观测与插件层。

---

## 2026-07-23：Direct 持续 Agent 会话

- 新增 Direct 运行模式：外观是单一持续 Agent 对话，底层复用单 Worker execution shell 和现有 ACP/session/storage 管道。
- Direct 使用 RawAgent prompt envelope，首轮与追问的 system prompt 均为空；不注入 Gold Band runtime/profile/hidden/output/repair 内容。
- 修复 completed run follow-up 生命周期：per-attempt live activity 区分真实 Starting/Running/CancelRequested 与 stale disk snapshot，页面重挂载后 composer、停止、耗时和 token 不丢失。
- 快速会话、runtime header、侧边栏和搜索完成 Direct 交互；Agent/model/permission 只在快速会话配置并按 workspace + Agent 记忆，运行模式管理仅保留工作流与 AUTO，Direct 历史使用 Agent icon 与 `lastActivityAt`。
- Direct 内部 `raw-agent` worker 不参与 profile 解析且禁止绑定 profile，避免角色解析阻断创建或向空 system prompt 注入 Gold Band 上下文。
- 回归范围包含 prompt、lifecycle、创建/config、前端 composer 状态、tab 顺序和 sidebar identity；合入前要求 Rust workspace、Web tests/build 与 `/chat`、Direct run deep link 实际验证通过。

## 2026-07-31：Direct 侧边栏活跃 turn 指示恢复

- 根因修复：Direct 用 Agent icon 替换 run 状态点且隐藏 run 子行后，侧边栏失去运行态入口；同时 completed run 上的后续追问不会把 `latestRun.status` 改回 running，因此不能在前端补一个基于 run status 的特例。
- 后端 `ConversationTaskRowVm.activity` 统一聚合 task 下 per-attempt live prompt activity 与首轮 runtime running 状态，覆盖 starting、accepted、running、cancel-requested 和 runtime-active。
- 前端在 Direct Agent icon 外使用轻量 CSS 旋转环；提交/停止返回的 canonical lifecycle snapshot 与 live lifecycle 事件同步更新 workspace、置顶两份 task 行，终态后恢复静态 Agent icon。
- 回归要求：Rust 单测固化 task root prompt activity 与 runtime fallback，Web 单测固化 lifecycle-to-sidebar 映射和 Direct-only 显示条件；通过 Web build/test、Rust 定向测试并 deep link 启动前端验证侧栏视觉。

---

## 2026-07-24：新会话搜索索引生命周期收敛

- 根因修复：侧栏继续以文件系统为权威事实源，SQLite 仍为派生搜索索引；task 创建和元数据更新统一由 `App` 核心生命周期刷新索引，不再由任务工作台或会话 UI 各自补调用。
- 跨 workspace 身份：task ID 只在项目内递增，SQLite schema v2 改用 `task_path` 作为主键；迁移保留现有索引行但不扫描旧任务，避免不同项目的 `task-001` 相互覆盖，删除也只清理目标路径。
- workspace 路由：项目 ID 统一复用 `GoldBandPaths::project_id`，Windows 对历史 drive letter 大小写差异兼容匹配；搜索命中后使用状态中已有的规范 project ID 组装路由，避免索引有结果但 workspace 解析失败后被过滤。
- 搜索 workspace 范围：会话搜索只覆盖 `conversationWorkspaces` 中显式存在的侧边栏工作空间，不再额外注入 `DesktopContext.repo_root`；不包含已移除或未注册的历史 workspace。允许的 task 目录在 SQLite FTS 排序与 `LIMIT` 之前过滤，避免范围外命中挤占可见结果。
- 中英文子串搜索：SQLite task FTS 升级为内置 trigram tokenizer；3 字符以上关键词支持标题、描述、需求正文任意位置匹配，1～2 字符关键词在 sidebar workspace 范围内使用字面包含匹配，修复“你好可命中但随便无法命中随便用askUserQuestion”的分词缺陷。用户输入统一按普通文本转义，多关键词使用 AND 语义，标题命中优先排序。
- 命中上下文展示：搜索接口新增 `matchPreview`，从真正命中的标题、描述或完整需求正文中截取上下文；短内容完整展示，只有长文本才在关键词前最多保留 10 个字符，避免短内容被误截断并保证关键词在单行内可见。关键词使用无底色、高对比 `foreground` 文字和轻量下划线高亮，兼容亮色与深色主题。
- 新数据范围：本次不扫描、不重建既有 `tasks` 索引缺口；修复发布后新建的会话，以及之后更新标题/描述的 task，可按标题、描述和 requirement 搜索。
- 可导航结果：会话搜索根据索引中的 `task_path` 解析 workspace，并从文件事实源补齐最新 Run；只返回能够形成 `projectId/taskId/runId` 路由的结果，点击后直接打开最近 Run。
- 错误语义：搜索索引不可用或查询失败返回结构化错误码，前端展示搜索失败，不再伪装成“没有匹配结果”。
- 回归固化：Rust 测试覆盖“创建 task 即可搜索、元数据更新刷新索引”、“搜索结果包含最新 Run”、“侧边栏 workspace 范围在 `LIMIT` 之前生效”和“随便/askUser/你好等中英文子串、短查询及命中摘要”；Web 测试覆盖 Tauri 搜索接口参数、搜索结果路由映射与关键词字面高亮，并要求桌面端完成“新建会话 → 搜索 → 查看命中上下文 → 打开”验证。

---

## 2026-07-24：会话页头身份信息收敛

- Direct 运行标题栏移除重复的 Agent、model、permission mode，仅保留目录按钮；Agent 身份统一由共享 ACP 会话信息栏承担。
- `AcpSessionVm` 增加由后端 provider 注册信息派生的 `adapterIconKey`，前端不通过展示名称猜测图标；未知 provider 使用通用 Agent 图标。
- 共享会话信息栏展示 Agent icon + 名称，移除会在会话中途变化的权限模式；session ID 支持点击复制，并通过自动消失的 Tooltip 提示复制成功。
- 回归要求覆盖 Direct 页头不再渲染旧配置元数据、共享 ACP 页头图标/权限隐藏/复制入口，以及 Web build 与 Direct deep-link 实际交互。
- Direct 在 session 就绪后不再渲染独立运行标题栏，而由 ACP 会话头组合标题、Agent/session 身份、原始帧与目录操作为单行；左侧身份组按自然宽度紧邻排列，Direct 标题不为透明编辑图标预留宽度，右侧操作组独立贴右，session 启动阶段仍保留运行标题占位，避免页头闪失。
- 会话标题编辑提示从 HTML `title` 切换到 shadcn Tooltip，统一 Direct、Workflow、AUTO 的主题样式与键盘可访问行为，不再出现 Windows 原生提示框。

---

## 2026-07-24：ACP 追问模型“不指定”语义修复

- Direct 发起会话的 Gold Band 合成模型选项由“默认模型”改名为“不指定”，英文为 `Unspecified`；提交仍使用空模型配置，不向 ACP 发送 Agent 模型 ID。
- attempt ACP session metadata 新增 `modelOverride`，与 Agent 返回的 `models.currentModelId / configOptions.currentValue` 分离。首次未指定模型时 override 为空，即使 Agent 报告 `currentModelId = default`，后续追问也不得把该值显式回传。
- 会话详情在 override 为空时展示“不指定”和 Agent 返回的完整模型目录；选择任意 Agent 模型后写入 override，并从该 session 的下拉列表中移除“不指定”。Agent 的 `default` 作为普通不透明模型 ID 原样保留。
- runtime continue、AI-DYNAMIC inner continue 和 ACP same-session prompt 统一只读取 `modelOverride`；具体模型继续通过 `session/set_config_option(model)` 应用，未指定则不设置模型并继承 Agent 环境配置。
- 回归覆盖 Agent `currentModelId = default` 但 Gold Band 未指定时续聊得到 `None`、用户明确选择 Agent `default` 时续聊得到 `Some("default")`、前端配置视图保持“不指定”和 Agent current model 分离，以及 Web build。

---

## 2026-07-27：ACP 权限模式“不指定”语义统一

- Direct、AUTO 与工作流编辑器中的可空权限模式统一将“默认 / 不设置”改名为“不指定”，英文统一为 `Unspecified`；会话创建前仍允许清回空配置。
- attempt ACP session metadata 新增 `permissionModeOverride`，与 Agent 返回的 `modes.currentModeId / configOptions.currentValue` 分离。首次未指定权限模式时 override 为空，即使 Agent 报告当前 mode，后续追问也不得把该值反推成 Gold Band 显式选择。
- 会话详情在权限 override 为空时展示“不指定”和 Agent 返回的完整权限模式目录；选择任意 Agent mode 后写入显式 override，并从该 session 的下拉列表中移除“不指定”，但仍允许在具体 mode 之间切换。
- runtime continue、AI-DYNAMIC inner continue 和 ACP same-session prompt 统一只读取 `permissionModeOverride`；未指定则不调用权限配置 API，继续继承 Agent 环境配置。模型与权限的 override/current 数据结构、显示和追问语义保持一致。
- 回归覆盖 Agent `currentModeId = default` 但 Gold Band 未指定时续聊得到 `None`、用户明确选择 Agent `default` 时续聊得到 `Some("default")`、前端配置视图保持“不指定”和 Agent current mode 分离，以及 Rust/Web 测试、Web build 和 Direct deep-link 实际验证。

---

## 2026-07-28：原始帧默认倒序与排序切换

- `AcpRawFrameQueryInput` 增加类型化 `asc / desc` 排序参数，后端默认 `desc`，以 append-only JSONL 行号作为稳定记录时序完成跨页排序；同一时间戳下不依赖不稳定的文本比较。
- Raw frames 筛选区复用 shadcn/ui `Select` 增加“最新优先 / 最早优先”，切换顺序、搜索或过滤后回到第 0 页；第一页、上一页和下一页文案按当前顺序表达实际时间方向。
- 破坏式替换旧的“最新页内升序”行为，不保留前端当前页反转或旧 `latest` 字符串兼容路径。
- Rust 接口层回归覆盖默认倒序、升序第二页、分页边界与返回排序枚举；Web build 和桌面端原始帧 deep link 验证控件默认值、切换结果及分页文案。

---

## 2026-07-24：会话工作空间状态与安全移除修复

- 根因修复：会话工作空间身份此前同时存在持久化 `conversationWorkspaces`、大小写不一致的 `projectId` key 和隐式 `DesktopContext.repo_root` 三条来源，导致 Direct 首轮可运行但追问按精确 key 报 `workspace.not-found`，移除时也可能删不中并重排相邻项。本次收敛为 `conversationWorkspaces` 单一列表来源，保留 workspace-scoped `App.paths.repo_root` 作为执行上下文，不再把桌面启动 workspace 当作会话成员。
- 状态迁移：新增 `stateSchemaVersion=1`，启动时一次性重新生成规范 `projectId`、按规范化路径去重，并迁移最后活跃工作空间、运行模式和置顶。规范 key 的运行模式覆盖历史大小写 key，确保用户已选择的 Direct Agent/model/permission 不被旧 Workflow 配置覆盖；迁移写入继续使用原子文件替换。版本命中后直接返回，二次调用也不改变 JSON。
- 统一解析：首轮创建、Direct completed-run follow-up、重跑、历史查看、权限/停止命令、附件、运行模式和置顶统一使用共享 resolver；Windows 历史 drive-letter 大小写可解析到状态中规范 ID，VM 与事件也继续使用规范 ID。
- 删除语义：后端先解析目标工作空间，再关闭其 ACP 连接并删除持久化列表项，同时清理关联 pins/run modes/last；未知目标返回结构化错误，任何 task/run/session 和工作空间文件都不删除。
- 删除交互：侧栏移除按钮先打开 shadcn/ui 确认框，展示工作空间名称并明确磁盘文件、历史会话保留；请求 pending 时禁止重复提交、取消和关闭，成功返回前不更新列表。删除当前会话所属工作空间后返回会话主页并选择后端 fallback。
- 回归固化：Rust 覆盖用户原始大小写冲突状态、规范 Direct 配置优先、迁移仅一次、显式 sidebar/search 范围、大小写 resolver 和关联状态清理；Web 覆盖确认门控、pending 单次提交、当前页 fallback 与最终工作空间为空。生产构建和 `/chat` 视觉验证通过。
- 编译契约修正：迁移代码使用的 `stateSchemaVersion` 固化到共享 `StateConfig`，补充非零版本 camelCase roundtrip、历史状态缺字段默认为 `0`、零值省略写回的单元测试，避免桌面 crate 与核心 crate 的状态模型再次不同步。
- Round 编号清理：删除指标功能遗留但从未接入的 `next_round_id` 目录扫描 helper；新 round 继续唯一地由当前 `RoundState.index + 1` 生成 ID，避免文件系统扫描与 runtime 状态形成双事实源。
- 全量回归修正：ACP timeline 计时测试原先把所有 fixture 事件写成相同 `seq=1`，解析进入 HashMap 后顺序不稳定，导致预期 11 秒而随机得到 1/8 秒；测试数据改为按落盘顺序生成单调递增序号，固化真实 timeline 接口契约，不修改生产计时算法。

---

## 2026-07-28：会话侧边栏相对时间边界修正

- 根因修复：原侧边栏以“周数小于 4”切换月份、以“月数小于 12”切换年份，但月份按 30 天、年份按 365 天取整，导致 28–29 天显示 `0mo`，360–364 天显示 `0y`。
- 领域收敛：相对时间格式化从 React 组件下沉到共享 `datetime` 模块，任务行与 run 行使用同一接口；继续保持侧边栏既有 `m/h/d/w/mo/y` 紧凑展示，不引入改变文案形态的第三方格式化依赖。
- 连续区间：不足 1 分钟显示“刚刚”，1–59 分钟显示分钟，1–23 小时显示小时，1–6 天显示天，7–29 天显示周，30–364 天显示月，365 天起显示年。
- 回归固化：前端纯函数测试覆盖所有单位切换边界、Unix 秒时间戳、未来时间与非法输入；生产构建和侧边栏实际展示验证通过后完成验收。

---

## 2026-07-29：用户反馈入口按渠道收口

- 仅 `wb` 渠道在顶栏显示「帮助」按钮；复用启动信息中的 `appInfo.channel` 贯穿 Shell 到 AppTitleBar，其他渠道及启动信息未就绪时不渲染入口。
- Web 回归测试分别固化 `wb` 可见与 `default` 不可见，避免后续渠道配置与 UI 能力再次脱节。

---

## 2026-07-29：ACP Elicitation 多行题干与跨版本结构兼容

- 根因修复：ElicitationCard 不再把 `params.message` 按换行和步骤下标切题；单题的上下文与实际问题整体展示，多题使用字段 description，通用 provider message 可隐藏。
- 协议边界：Rust 使用官方 `agent-client-protocol-schema 1.6.0` 的 `CreateElicitationRequest` 反序列化并持久化完整请求，timeline 保留 mode、scope、session/tool identity、schema 与 `_meta`。
- 版本兼容：按 schema shape 支持 Claude Agent ACP 0.44 全局 `customAnswer`、0.45.1 `question_n_custom` 和当前 `_askUserQuestionCustomAnswer` 元数据，不要求用户机器上的旧 Agent 同步升级。
- 展示能力：选项 description 与 Claude preview 元数据保持结构化渲染；普通文本字段不再被猜测为首题自定义答案。
- 回归固化：Rust 覆盖生产 0.44 fixture、pending roundtrip 和完整 timeline request；Web 覆盖多行题干、三类自定义答案、选项元数据及刷新恢复，并要求生产构建和 ACP 会话实际验证通过。

---

## 2026-07-30：PR #81 合并修复与反馈安全边界收敛

- 合并策略：保留单一 PR 与原分支，合并最新 main 后同时保留反馈渠道和 main 的 avatar、ACP elicitation、terminal failure 等能力，不拆分提交组。
- 反馈信任边界：破坏式删除 `sessionWorkspace` / `screenshotPaths` command 契约，改为 `projectId + taskId` 后端解析和截图 File bytes；task id、canonical root、逐文件路径与 symlink 规则统一校验。task id 的路径分隔符校验显式覆盖 `/` 与 `\\`，不依赖 Windows/Linux 的 `Path` 解析差异。
- 工作空间状态清理：移除 workspace 时以请求 ID、持久化 ID、路径重算 ID 组成身份别名集合，统一删除 run mode、pin 与 last workspace 引用，固化跨平台大小写差异下的回归测试。
- 资源生命周期：使用 image/walkdir/tempfile/zip/ReaderStream；截图验证后统一重编码 PNG，任务 ZIP 写临时文件并流式上传；统一限制描述、截图、归档未压缩/压缩/文件数、日志和总请求大小。
- 渠道能力：`feedbackEnabled` 从 channel JSON 编译到 `AppInfoVm`，前端只透传 boolean，后端二次门控；不再硬编码 `channel === wb`。
- 错误协议：补齐 disabled、session-not-found、attachment-invalid、payload-too-large 等结构化错误码；网络原始错误只写 metrics.log。
- MCP 范围收口：transport、Streamable HTTP 和 per-Agent 兼容性由独立 MCP 方案统一维护；本次删除 provider 层按 provider ID 硬编码 transport、预过滤 server 和 attempt warning 的重复实现，避免与 MCP 管理域形成双重事实源。
- 配置规范化：stale Agent config option 使用纯函数清理，validate 不再 mutation 输入；Direct/AUTO 提交和能力刷新使用规范化结果。
- 回归要求：Rust workspace、桌面 crate、Web 全量测试、生产构建、default/wb 渠道编译与 wb UI 实际验证全部通过后才允许推送原 PR 分支。

---

## 2026-07-30：Streamable HTTP MCP 协议与 session 生命周期修复

- 根因修复：废弃“读取完整 HTTP body 后取第一条 `data:`”的错误模型；Streamable HTTP SSE 改为按 event 增量解析，并按 JSON-RPC request id 等待对应 response，允许服务端在此前发送 request、notification、keepalive 或其他 response。
- SSE framing：多条 `data:` 按标准使用换行拼接，comment 不产生消息；目标 response 到达后立即返回，不依赖服务端关闭 SSE 连接。
- session 状态：`Mcp-Session-Id` 与协商后的 `protocolVersion` 统一由客户端管理；后续 notification、tools/list 与 DELETE 均携带协商版本和 session header。
- session 恢复：携带 session 的请求收到 `404` 后，不单独重放失败请求，而是清除旧状态并完整重走 initialize → notifications/initialized → tools/list；连续失效则停止重试并返回错误。
- 资源释放：健康检查与工具发现属于短生命周期操作，完成或失败后均 best-effort 发送 HTTP DELETE；`404/405` 视为已释放或服务端不支持主动释放。
- HTTP 方法安全：禁用自动重定向，避免 301/302 将 MCP POST 降级成 GET；要求配置最终 endpoint URL。
- UI 修复：Agent 兼容性状态的 Tooltip 使用非 disabled 包装触发器，支持/不支持状态仍可 hover 查看说明。
- 回归固化：Rust 单元测试覆盖多行 SSE、前置 notification/错误 id、目标 response 到达但连接仍保持、session 404 后重新握手、协商版本透传和最终 DELETE。

---

## 2026-08-01：会话主页 composer 自动增高与宽度收敛

- 根因修复：会话主页原先使用固定 `min-h-24` 的原生 textarea，而会话追问已使用 prompt-kit 自动尺寸输入，形成两套不一致的输入生命周期；主页改为复用 `PromptInputTextarea`，不新增独立 autosize 实现。
- 布局契约：首页主内容收窄为 `max-w-3xl`；正文区以 56px 为初始最小高度，按内容增长到 320px 上限，未到上限隐藏滚动条，超过上限后固定高度并在正文区内部滚动，工具栏保持稳定。
- 光学居中：主页横向继续相对可用主区严格居中；纵向在居中容器底部增加 64–80px 响应式布局留白，使内容组上移约 32–40px。该留白参与 flex 布局计算，不使用 transform，避免视觉位置与真实布局位置分离。
- 回归固化：共享自动尺寸函数覆盖短文本、中等文本和超限文本，布局配置测试固定主页宽度、最小高度与增长上限；要求 Web 测试、生产构建和 `/chat` deep link 实际验证通过。

---

## 2026-08-02：macOS 原生窗口控制恢复与 chrome 所有权收口

- 根因修复：Rust 启动阶段已为 macOS 恢复 native decorations、Overlay title bar、hidden title 与 shadow，但 Web reveal 流程随后无条件调用 `setDecorations(false)`；前端又按 macOS 平台隐藏自绘窗口按钮，最终形成只保留安全区而没有 traffic lights 的空白标题栏。
- 生命周期边界：窗口 decorations、title bar style 与 native shadow 统一由 Rust/Tauri 宿主管理；Web 层只负责同步主题 surface、显示窗口以及渲染平台对应的控制入口，不再修改 native chrome，也不再持有 `allow-set-decorations` 权限。
- 布局保持：不修改共享标题栏组件结构；macOS 固定“traffic lights 安全区 → 品牌 Logo/标题 → 左侧栏开关 → 弹性拖拽区 → 右侧栏开关”，Windows/Linux 继续使用右侧自绘最小化、最大化/还原与关闭按钮。
- 回归固化：新增窗口 chrome 所有权契约测试，验证 Rust macOS 配置、Web reveal 无 decorations mutation、能力最小化和标题栏元素顺序；要求相关 Vitest、Web 生产构建与 Rust 桌面测试通过，并在 macOS 安装包上验收 traffic lights、拖拽及左右侧栏按钮点击。

---

## 2026-08-02：四主题滚动条低对比度校准

- 根因修复：现有滚动条组件和全局消费路径保持不变，修正主题语义 token 将品牌色与辅助文字色混成不透明深色的问题；不为侧栏或单个滚动容器增加局部覆盖。
- 主题策略：四套主题统一改用中性 `foreground` 透明叠加，轨道维持 3%–4%，静止 thumb 维持 16%–20%，hover thumb 维持 26%–32%；终端黑略高于其他主题以保留可发现性，浅色主题不再呈现品牌蓝滚动条。
- 接口一致性：全局原生滚动条、`.gold-themed-scrollbar` 和 shadcn `ScrollArea` 继续只消费 `gold-scrollbar-*` token，组件尺寸与交互范围不变。
- 回归固化：扩展滚动条 Vitest，逐主题验证中性低透明 token、静止/悬浮层级递增，以及滚动条 token 不再依赖 `primary` / `muted-foreground`；要求 Web 测试、生产构建和四主题实际页面核验通过。

---

## 2026-08-03：桌面开发监听范围收口

- 根因修复：根 Cargo package 与 `src-tauri` 构成 workspace，Tauri dev watcher 会监听 workspace package；此前没有仓库级 `.taurignore`，导致 `docs/` 和根 README 等非运行时文件变化也触发桌面应用重建。
- 监听边界：采用 Tauri 官方 `.taurignore` 扩展点，以 Gitignore 语义统一排除 `docs/` 和根目录 `README*.md`；不关闭 Rust 热重载，也不修改已经以 `web/` 为 root 的 Vite 监听范围。
- 回归固化：新增开发监听配置契约测试，使用 Git 的 ignore 匹配接口验证嵌套文档与中英文 README 均被忽略，同时 Cargo、Rust、Web 源码和 package 配置继续可观察。现有开发进程需重启一次后应用新规则。

---

## 2026-08-04：会话初始附件解析与历史消息投影一致性

- 根因修复：附件扩展名白名单声明支持 `.jsonl`，但 provider 文本类型判断遗漏该扩展名，导致文件已保存到 task `authoring/inputs/`，却没有进入 ACP content block、`PromptBundle.attachment_metas` 和用户消息 `raw.attachments`；图片正常、普通 `.jsonl` 附件消失。
- 数据设计：删除扩展名白名单、MIME 映射和 image/text 判断三份重复定义，改为统一附件格式注册表；`supported_attachment_extensions()`、provider resolver 与消息元数据共同消费该注册表，`.jsonl` 作为 `application/json` 文本资源发送。
- 历史恢复：`SessionMode::New` 根分支的 session ViewModel 从 task `authoring/inputs/` 恢复旧 timeline 首条 Gold Band 用户消息缺少的 task 输入附件，按 `task-inputs/<name>` path 去重，不改写原始 timeline，也不污染带 `promptId` 的后续追问。
- 接口验收：Rust 单测固定“所有公开支持扩展名均可解析”、`.jsonl` 同时生成文本 content block 与附件元数据、历史首条消息补全且不重复/不进入后续消息；前端附件数组回归固定同一用户消息同时保留图片与普通文件。目标 `task-158/run-001` 的真实落盘数据验收确认历史投影结果同时包含 `image.png` 与 `acp.raw.jsonl`。
- 展示完善：消息组件按附件媒体类型派生图片组与普通文件组，固定渲染为“图片行在上、文件行在下”，同类附件各自行内换行；普通文件使用内容宽度的紧凑 pill，不与固定尺寸图片缩略图混排或共同拉伸。
- 前端回归：纯函数测试固定混合输入的分组结果；DOM 测试固定两个附件行的顺序、内容隔离，以及普通文件按钮的 `w-fit` / pill 样式契约。

---

## 2026-08-06：AI-DYNAMIC 候选 Agent 原生权限配置

- 根因修复：删除 AI-DYNAMIC 节点级共享权限字段以及产品侧 `read_only / ask / full_access` 枚举和 `permissionModeMapping` 中央映射，消除新增 ACP Agent 必须补 provider 映射的扩展阻塞。
- 数据契约：dynamic 控制面保存 `bootstrapProvider / bootstrapModel / acceptanceModel / permissionMode`，其中分发与验收模型都来自初始分发 Agent 目录，bootstrap/merge/acceptance 共用原生权限；每个动态候选 Agent 继续保存 worker 自己的模型、config options 和原生 permission mode id。AUTO 与 workflow AI-DYNAMIC 使用同一结构。
- 路由契约：dynamic strategy 的 `dynamic-node-completion` 只有 worker 选择 Agent/provider，merge / acceptance 禁止输出 provider，所有节点都禁止 `model / permissionMode`；runtime 给 worker 注入候选配置，给 bootstrap/merge/acceptance 注入控制面配置。会话建立后继续允许通过 ACP session config 实时切换模型与权限，实时 override 不改变初始化配置。
- UI 交付：AUTO 的“可选动态 Agent”和工作流 AI-DYNAMIC Inspector 在每个候选 Agent 旁共同展示模型与原生权限选择；bootstrap 与 fixed 配置同步成对展示；动态 AUTO composer 不再展示共享权限入口。
- 回归固化：Rust 接口测试覆盖 AUTO 配置到 DSL 的模型/权限传递、原生权限 doctor 校验以及 provider-only output contract；Web 测试覆盖候选权限回显、提交规范化与非法原生权限阻断，并要求 Rust/Web 构建和目标页面交互验收通过。

---

## 2026-08-06：AUTO 失效工作流引用自修复

- 根因修复：历史或异常写入的 AUTO `allowedWorkflows[].workflowId` 无法在当前工作流模板库中解析时，旧页面只报阻断错误而不展示该选项，用户无法移除它，导致保存和另存模板全部不可用。
- 数据策略：加载运行模式后仅清理“当前模板库完全不存在”的工作流 ID 和“当前角色目录完全不存在”的 profile ID，并立即合并回写 project 级 AUTO 配置；当前已选 AUTO 模板在被加载时才校验并以单模板更新写回用户级 `auto-templates.json`，未选模板不扫描、不改写。存在但本身不可选的工作流继续保留，由现有重复 ID、空 ID 与嵌套 AI-DYNAMIC 校验明确阻止，避免静默改变有效但需要人工处理的配置。
- UI 与验收：清理后在 AUTO 模板操作行下显示黄色警告横幅，分别说明工作流和角色移除数量，约 5 秒自动消失，切换模板时立即清除；通知的消失规则只由消息类型统一决定，warning 不能被调用方覆盖为常驻。保存继续可用。Web 单元测试覆盖两类缺失引用剔除、保留有效引用与 warning 时长。

---

## 2026-08-06：AUTO 模板分布式身份

- 根因修复：AUTO 模板曾从显示名称生成 slug ID；中文名称会退化成空串或少量数字，ID 身份与名称耦合，只能使用顺序后缀处理冲突。
- 数据策略：新建 AUTO 模板改由后端生成 `auto-template-<uuid-v4-without-hyphens>`；名称只用于展示和重名校验。导入发生空 ID 或冲突时同样生成该分布式 ID；已有 ID 不迁移，避免破坏已保存的 `activeTemplateId` 引用。
- 回归固化：核心单元测试验证 ID 与名称无关、具备规范前缀和 UUID 长度、连续生成不重复；浏览器预览 API 使用相同 UUID 策略。

---

## 2026-08-09：桌面生命周期与 macOS 跨平台能力收敛

- 根因修复：将“关闭主窗口”和“退出应用”拆为两个领域动作，由 `DesktopLifecycleCoordinator` 统一管理 `Running / ClosingMainWindow / AwaitingFrontend / Cleaning / ReadyToExit`。macOS 红色关闭只销毁 WebViewWindow 并保留 runtime，Dock 重开显示或从 Tauri 配置重建；Windows/Linux 关闭、Cmd+Q、菜单退出和 updater 退出共用保存握手与 15 秒有界清理。
- 进程治理：引入 `command-group 5.0.1` 的 `ManagedProcessGroup`。Windows 使用 Job Object 和 `CREATE_NO_WINDOW`，Unix 使用进程组 TERM→KILL；ACP、MCP stdio、Agent doctor 与登录 Shell PATH 探测已迁移，正常退出不再散落终止单个 PID。
- 跨端集成：工作空间与会话目录 reveal 统一使用官方 `tauri-plugin-opener`；通知点击统一进入 Rust 待导航队列并恢复主窗口，Windows Toast 保持现有展示，macOS/Linux 使用 `notify-rust` typed response，消除窗口重建时事件早于监听器的竞态。
- 发布策略：macOS 默认由 Tauri bundler ad-hoc 签名，同一 release 流水线继续产出 arm64/x64 DMG；Apple 凭证全空、部分、完整三种配置由脚本校验，完整时在同一 `tauri-action` 签名公证，构建后严格验证 `.app` 签名。产品不增加 unsigned 分支、文件名后缀或额外提示。
- 临时安装闭环：Apple Developer Program 凭证未就绪期间，仓库脚本只服务带 `.sha256` 的新 macOS Release。两条发布流水线在资产汇总阶段为 DMG、macOS updater archive、Windows installer 和 Linux packages 流式生成 sidecar；安装脚本不依赖 Python/jq，固定校验 sidecar、DMG、App 名、bundle identifier 与 codesign，并通过同卷暂存、旧 App 备份和失败恢复完成替换。历史 Release 不回填 checksum，也不进入弱校验 fallback。
- 详细设计与验收见 `开发计划/生命周期整理/桌面生命周期与跨平台集成重构.md`。

---

## 2026-08-09：系统通知跨项目导航身份修复

- 根因修复：通知生命周期事件和点击载荷原先只携带项目内局部编号；多个 workspace 同时存在 `task-001/run-001` 时，前端会按 taskId 选择第一个项目并进入错误 run。通知定位协议现将 `projectId` 设为必填字段，从 runtime 生命周期事件贯穿核心通知模型、Toast action、待导航队列和前端 deep link。
- 去重契约：canonical dedup key 增加 project 维度，不同 workspace 即使 run/round/node/attempt/turn 全部同名也不会互相抑制通知。
- 前端路由：删除通过 sidebar `tasksByWorkspace` 按 taskId 反查第一个项目的模糊 fallback；当前 run 复用判断也必须同时匹配 project/task/run 完整身份。
- 回归固化：Rust 测试覆盖跨项目同局部 ID 的通知身份与去重隔离、Toast action projectId roundtrip；Web 测试覆盖同 task/run 不同 project 时只匹配通知指定项目。

---

## 2026-08-09：Direct 自动队列完成通知合并

- 根因修复：Direct 待发送队列此前为每个自动发送的成功 turn 创建独立 OS 通知，Windows 会把多个 Toast 串行排队，形成“点击一条后下一条继续弹出”的干扰。首版仅延迟 `turn-queued-*`，遗漏了触发同一批次的首条普通 `acp-prompt-*`，因此仍会先出现一条通知。通知策略现完整绑定队列的实际 claim 边界，不依赖 prompt id 前缀、Windows Toast 展示顺序或平台专用替换 API。
- 生命周期契约：`AcpPromptLifecycleEvent::Finished` 携带稳定 `promptId`；Direct 首轮成功 `RunCompleted` 也只更新运行状态，通知延后进入相同队列决策。`AcpTurnFinished.batchProgress` 以 `completedReplyCount + continues` 表达批次进度：实际领取后继时累计并抑制中间成功，终点携带完整累计数后清理；计数 1 显示“回复完成”，大于 1 显示“已连续回复 X 条”。失败立即通知并清理批次，权限、elicitation、运行异常及不同会话完全不受影响。
- 回归固化：核心 prompt queue 单测固定累计、终点重置和失败清理；通知模型单测固定多条文案；桌面生命周期单测固定连续事件的 `batchProgress`，通知策略单测固定 Direct 首轮延后、中间成功不通知、末尾成功通知和失败不抑制，并要求通知、桌面端测试和格式检查通过。

---

## 2026-08-09：非 Windows 通知响应编译契约修复

- 根因修复：`notify-rust 4.18` 的 `ResponseHandler` 接收 `&NotificationResponse`，桌面适配器错误地按值推断参数，导致 Linux 与两个 macOS release job 在 Rust 编译阶段同时失败。适配器现显式遵守借用签名，并先把第三方响应映射为内部 `Navigate / ClearDedup` 处置后再操作导航队列和 dedup。
- 回归固化：新增响应分类与 borrowed `ResponseHandler` 契约单测，覆盖正文、view、其他 action、reply 和 closed；PR checks 保留完整回归，两条 release 流水线的多平台构建必须等待 Linux `cargo check --workspace --all-targets` 通过。发布预检只验证平台代码可编译，不执行全量业务测试，避免无关的平台断言阻断打包。

---

## 2026-08-06：Runtime artifact 约定后置

- 根因修复：原实现把“业务执行”和“runtime 控制结果归一化”压在同一个 prompt turn，导致 agent 在工作开始前就被结构化 artifact 协议约束，自然业务回复与控制 JSON 相互污染。保留现有 `output_contract` 作为 runtime 控制契约，并新增 `PostTurnProjection / InlineControl` 发射模式，不拆出第二套 contract 领域。
- 执行契约：普通 workflow worker 与 AI-DYNAMIC 的 worker / workflow invocation / acceptance 先以 Conversation 策略完成可见业务 turn，再复用同一 ACP session 发送隐藏 `RuntimeFinalize` prompt 生成 artifact；AI-DYNAMIC bootstrap dispatcher 的职责就是分发，继续使用 `InlineControl` 在首轮接收并输出完整动态协议。Direct / `RawAgent` 不变。
- 生命周期：业务 turn 成功后先原子写入 `artifact-emission.json(finalizing)`，再开始隐藏 finalize。纯继续、进程恢复和自动重试观察到 `finalizing` 时跳过已完成的业务执行并继续 finalization；无 phase 时仍按业务 turn 恢复。若用户在 finalize 暂停边界选择继续并发送，则先原子改写为 `business-turn` 并执行新的用户业务 turn，成功后再回到 `finalizing`；该业务 turn 再次中断时不得直接跳 artifact。finalize 输出 repair 只修复 artifact，不重新执行任务；损坏或版本不支持的 phase 不允许静默回退。
- 提示词与观测：中英文 finalize 模板统一放入 `src/prompts/<language>/runtime/artifact_finalize.md`；业务、隐藏 finalize 与隐藏 repair 的稳定 system prompt 均不暴露 PostTurn schema，完整协议只进入隐藏 user prompt；隐藏 timeline reason 区分 `artifactFinalize` 与 `invalidOutputRepair`。
- 回归固化：Rust 单元测试覆盖发射模式到 ACP 输出策略的映射、PostTurn 业务/finalize/repair system prompt 不含 schema、隐藏 finalize user prompt 内容与 reason、durable finalizing 恢复、workflow 默认后置，以及 AI-DYNAMIC bootstrap/普通 worker/acceptance 的模式分流。

---

## 2026-08-10：工作流停止后 Runtime 控制与自由会话分离

- 根因修复：将“Agent turn 是否由 Runtime 消费”从 prompt 内容与节点暂停状态中抽离为 invocation 级 `RuntimeControlled / NonRuntimeControlled`。普通消息不会再因为回复结束而读取 artifact、计算 outcome 或推进 workflow。
- 交互收敛：`Paused + ProcessInterrupted` 不新增状态；composer 保持普通聊天，并提供独立继续动作。发送按钮与 Enter 固定走 NonRuntime ACP prompt；没有可发送输入时继续动作显示“继续工作流”，调用 `continue_conversation_runtime` 并发送隐藏 `RuntimeResume`，不创建可见用户消息；存在可发送输入时显示“继续并发送”，以一次 continue command 原子提交用户输入与恢复意图，用户气泡只显示用户输入。
- 边界提示：Workflow/AUTO 的中英文基础 runtime system prompt 预先声明用户主动打断并转向其他内容时，在 Runtime 明确恢复前无需遵守 artifact 输出语义；中断期间针对当前任务的最新用户指引在恢复后继续有效，可调整任务内容、交付结果与角色流程，但不能覆盖 artifact contract、文件规则及安全边界。AI-DYNAMIC 通过既有 system 组合自然继承且不重复提示。停止后的普通消息保持用户原文，不再追加一次性 suspended hidden context；显式继续的隐藏 `runtimeControlResume` 只用一句短提示声明 Runtime 控制与当前输出契约恢复，不重复 system 规则，也不自动恢复中断前的角色流程。
- artifact 完整性：PostTurn finalize 中断输出一律不可信；`artifact-emission.json(finalizing)` 的纯恢复只跳过上一业务 turn并重新请求完整 finalize；继续并发送原子切换为 `business-turn`，先执行用户新消息再重新 finalize。InlineControl、PostTurnProjection 与 AI-DYNAMIC 精确 leaf resume 继续复用现有 contract 和 scheduler。
- 并发与接受边界：`WorkflowContinued` 只在 accepted prompt event 落盘后以 source transition CAS 提交，迟到 resume 不覆盖新 stop。固定工作流 continue 使用 per-run starting lease 拦截双击，且不持有全局锁等待 Agent turn。
- 性能收口：legacy cursor 缺失时只回扫 timeline 一次并持久化 negative cache；cursor 并发写入使用固定 64 路路径哈希短锁，不维护随 attempt 数增长并在热路径全表清理的锁注册表。Direct / `RawAgent` 首轮直接派生为 NonRuntimeControlled。
- 回归固化：Rust 覆盖 control cursor、NonRuntime 重复 stop、accepted 后 resume commit、stale resume CAS、legacy negative cache、停止后普通 prompt 原文透传、固定 continue starting lease、Direct 首轮、stop probe、NonRuntime contract policy 与 interrupted PostTurn，并固定 `finalizing + RuntimeResume` 只恢复 finalize、`finalizing + UserMessage` 进入 `business-turn`、二次停止后仍先恢复业务 turn；Provider prompt bundle 覆盖 PostTurn 业务 turn 不暴露 output DSL，以及继续并发送时 provider 组合 prompt 与 UI `prompt_display` 分离；Web 覆盖 paused action + 普通 composer、发送/Enter 不恢复、继续并发送只产生一次 Runtime continue、失败恢复草稿、accepted optimistic processing 收敛、`show=false` hidden 段不生成气泡入口，以及旧 `interrupted-input/runtime-continue` 语义删除。
- 继续资格收紧：通用 continue 只恢复 `ProcessInterrupted / RuntimeAbnormal`；manual check 等待保持 NonRuntime 并只由成功/失败按钮推进，permission、elicitation/waiting 与 ErrorBlocked 不能被通用入口绕过。fixed 与 AI-DYNAMIC 复用同一领域判定。
- durable acceptance：`runtime-continue-started` 改为 Running 状态落盘后的启动握手结果；启动前失败同步返回结构化错误。握手后意外失败只对原 active attempt 做 CAS 收敛并刷新权威 lifecycle，迟到错误不覆盖用户 stop/完成/attempt 切换；AI-DYNAMIC 同时回收 re-arm leaf 与 starting registry。
- 性能约束：握手使用一次性 channel、无轮询；fixed starting lease 在 Running durable fact 后释放，不跨 Agent turn。失败状态 CAS 复用固定 64 路短锁和 dynamic graph lock，只覆盖小型状态文件写入。
- workspace 一致性：AI-DYNAMIC 新增持久化 `Executing / PreparingWorkspace` 内部阶段，checkpoint、fork、merge 前准备与 release 继续在 dynamic graph lock 内完成。准备期间 UI 显示“正在准备开发环境…”，用户点击停止后沿用“正在停止…”并等待临界区结束；已创建 worktree 保留，continue 复用原 workspace tree。阶段开始只写 `dynamic-run.json + graph.json`，不重复重写全量分文件，也不新增轮询或 Agent turn。
- stop boundary：外层 stop 落盘后，任何旧 dynamic execution 的迟到成功结果都不能恢复 Runtime；完整合法 completion 也必须等待用户显式 continue 建立新 execution generation。接口级回归覆盖 phase 持久化、停止 pending、临界区释放后 Paused、workspace 保留与前端 stopping 优先级。
- 普通追问门禁修复：删除 conversation submit 中复用 `runtime_continue_required` 的旧 preflight；Workflow/AUTO 的 `Paused + ProcessInterrupted` 可以同时具备 NonRuntime 普通发送与显式 continue 两项能力。普通发送仍只在 attempt 当前由 Runtime 控制时拒绝，接受后不得改变 run/node 暂停事实或消费 continue 资格；Rust 接口回归固定两项能力相互独立。
- 停止/继续交互收敛：session tree/header 的 starting、sending、cancelling、cancel-requested 状态统一投影为可见运行/暂停语义点，停止后不再出现与深色背景融为一体的 neutral 点；continue command 的 durable active lifecycle 立即局部收敛 composer、session tree 与 sidebar task/run 摘要，使“正在继续”直接切换为“停止”且两级侧栏立即变为 Running，不等待下一节点或父级刷新。session tree/header 的 Running 点复用 sidebar 的 reduced-motion-safe 呼吸动画，不增加轮询或独立动画状态。

---

## 2026-08-11：管理页 Header 表面统一

- 根因修复：Agent、上下文和运行模式管理页共用的 `PageHeader` 只有信息型详情页样式，固定叠加大标题、半透明独立背景、模糊和底边界；管理页因此被切成突出的顶栏与主体两块，而定时任务管理页已经采用更符合桌面产品心智的同底紧凑页头。
- 组件契约：共享 `PageHeader` 增加类型化 `default / integrated` 变体，以及可选 `icon`、`navigation` 槽位。`default` 保持详情页现有信息密度；`integrated` 集中定义紧凑标题、与标题组视觉中心对齐且使用 `text-foreground` 自动适配明暗主题的 20px 语义图标、24px 水平内边距、32px 顶部内边距、无一级导航时约 28px Header 到主体间距、透明同底和无分割线规则；Agent、上下文、运行模式、定时任务四页统一显式消费。宽屏标题组和操作区统一顶部对齐，操作区高度不参与标题纵坐标计算；存在一级 line Tabs 时由共享 `navigationRoot + PageContent(after-navigation)` 将下划线到主体收紧为约 12px，避免层级边界与标题留白重复。
- 交互收敛：三个管理页分别复用侧栏语义的 `Bot / Library / Route` Lucide 图标；Agent 页保留标题右侧刷新与新增操作并统一为紧凑 shadcn Button；上下文一级导航进入 Header 的 `navigation` 槽位并改用 shadcn line Tabs，移除独立灰底分段条和整行边界。角色、MCP、SKILL 的刷新入口统一为 32px `RefreshCw` 图标按钮，复用 shadcn Tooltip 替代浏览器原生 `title` 提示，并保留本地化 `aria-label`；运行模式业务分段与项目选择仍留在主体工具区，不改变数据生命周期。
- 回归要求：前端契约测试固定 `integrated` 不包含背景、模糊或底边界，标题为紧凑层级，图标槽具备隐藏装饰性语义且图标标题组使用垂直居中布局，并固定四页图标映射、导航后间距及上下文 line Tabs 结构；上下文额外固定角色、MCP、SKILL 三处内层 Tabs 均消费 `EntitySection`，并覆盖 shadcn CardHeader 的边界默认 padding，将 tab 行 CSS 底部 padding 统一收紧为 4px、实际 Tab 到 Header 底边界空间约 12px。同步执行目标测试、全量 Web 测试、类型检查、生产构建和正常/窄宽度 deep-link 验收。

---

## 2026-08-11：定时任务 Header 与工作区筛选稳定化

- 信息收敛：定时任务管理页删除自建 `<main> + <header>` 壳，直接复用其他管理页的 `Page + PageHeader(integrated)`；同时删除“按计划执行并追踪最近一次运行”解释性副标题及中英文废弃文案，只保留 `AlarmClock`、标题和任务数量，让页面级信息密度、顶部边距和图标标题对齐完全由共享组件保证。
- 根因修复：原生 `<select>` 的宽度由最宽 option 决定；任务异步加载后追加真实工作区选项会触发固有宽度重算，导致工具栏横向跳动。工作区筛选改用项目现有 shadcn/ui Select，通过 `w-28` Tailwind 尺寸 token 固定触发器宽度，并显式渲染当前筛选名称以保证 SSR 首帧与 hydration 后一致；选项集合变化不再影响布局。
- 主题一致性：审计全前端 `text-primary` 使用语义，将管理列表、会话侧栏、会话标题、创建摘要、配置面板和执行历史中的 `AlarmClock / CalendarClock / ListChecks`，以及文件变更、文件/资产查看、源码管理、右侧工作区入口中的静态功能图标收敛为 `text-foreground`；任务列表图标容器使用 `bg-foreground/10`。状态图标、链接和选中态继续保留语义色。会话标题标识移除硬编码中文与原生 `title`，改用中英文 i18n 和 shadcn Tooltip。
- 经验沉淀：在 UI 交互规则真源新增“图标语义与主题适配”，将静态标识、状态色、品牌图标三类颜色决策、成对主题验收、同语义跨页面一致性及 Tooltip 约束固化为全局前端设计规则，后续不得再将 `primary` 作为通用图标色使用。
- 回归要求：前端单元测试固定副标题不再渲染、原生 select 不再回流、工作区筛选消费固定宽度契约；生产构建后在加载前、加载后、窄窗口和恢复宽度四种状态验收工具栏稳定性。

---

## 2026-08-11：侧边栏上下文与运行模式图标收敛

- 视觉根因：上下文原 `Boxes` 与运行模式原 `Workflow` 都包含多个节点和密集交叉线，在同组 `Bot`、`AlarmClock` 等单主体 Lucide 图标旁视觉重量偏高，选中态下尤其显得碎且拥挤。
- 实现方案：继续复用当前 `lucide-react`，上下文改用 `Library` 表达可复用资源库，运行模式改用 `Route` 表达工作流 / AUTO 的执行路径；导航尺寸、描边、间距、选中态和行为保持不变，不引入自研 SVG 或新依赖。
- 回归要求：前端契约测试固定两个导航入口的语义图标映射并禁止旧图标回流；生产构建通过后使用内置浏览器验证上下文和运行模式的普通态、选中态、窄窗口与恢复宽度。

---

## 2026-08-11：状态、生命周期与数据完整性工程规则

- 将历史身份串用、状态回退、恢复丢失、半完成写入和局部坏数据拖垮整体等问题收敛为统一工程规则：稳定身份与完整作用域、canonical state 单一权威、`status/outcome` 分离、异步单调合并、durable/transient 分层、原子幂等写入、局部失败隔离、能力发现和资源信任边界。
- 根 `AGENTS.md` 只提供强制路由，详细约束由 `docs/gold-band/rules/state-lifecycle-and-data-integrity.md` 作为唯一真源；runtime 总览增加对应边界入口。
- 新增经验沉淀机制：Bug 或设计修正完成并验证后先判断复用价值并检索现有规则；只有向用户说明原则与收益并获得明确同意后才可写入，规则必须精简、可执行、可验收且不复述具体问题。
 - 2026-08-12 RunMode 边界修复：新增 `ConversationRunMode::is_orchestrated()`，统一由 Workflow/AUTO 获得 Runtime continue 资格，Direct 即使底层容器暂停也只保留 NonRuntime 普通发送。后端 continue command 与 lifecycle projection 同时执行该领域判定，前端删除 stop 后本地伪造的 action，并把“继续工作流”移入 prompt-kit composer 的发送 action 行。Direct 首轮提前停止、manual check、AI-DYNAMIC leaf、错误标题与 i18n 占位符均由接口/组件测试固化。

## 2026-08-12：历史 AUTO dynamic graph workspace catalog 迁移

- 根因修复：AI-DYNAMIC workspace catalog V2 曾在 graph 仍标记 `0.1` 时直接替换 `workspace/workspacePath` 为 `workspaces + workspaceId`，历史 graph 因反序列化失败而被会话树读取路径忽略。现将 graph schema 单独提升为 `0.2`，所有生产消费方统一经过版本化存储边界，不在 Conversation VM 或前端增加特例。
- 迁移契约：首次读取旧 `0.1` graph 时确定性构建 main/runtime workspace catalog 和 group workspace 拓扑，校验后使用原子替换写回；并发读取按文件串行化，当前 `0.2` 与第二次读取均不改盘。dynamic run、node、attempt/session locator 身份保持不变，无法证明仍安全可用的历史 workspace 标记为 `released`。
- 性能与回归：不做启动全量扫描；单图首次迁移为 `O(nodes + groups)` 内存转换、按旧 worktree 数量进行有界 Git 校验并执行一次原子写入，后续恢复普通读取成本。core 测试覆盖磁盘一次性/幂等写入、当前版本 no-op、未来版本拒绝、readonly/worktree fanout 拓扑与 released 降级；桌面 Conversation VM 接口测试覆盖旧 AUTO graph 恢复 dynamic session leaf、默认选中 key 并落盘 `0.2`。
- 编译契约修正：Git HEAD 查询失败分支按 `Result` 接收并忽略错误值后回退到稳定的 `legacy-unknown`，保持原迁移数据、接口、I/O 次数和复杂度不变。

## 2026-08-12：会话 disclosure 密度、处理动画与停止收敛

- 隐藏 system/runtime context 的展示投影改为先分组隐藏段、再合并可见片段，并只在展示层移除隐藏段边界产生的前导空行；保持 provider prompt 原文不变，消除折叠项与需求正文之间的模板 spacer。
- 活动摘要与 composer/compact 用量栏复用同一个 CSS 边框圆环组件，统一 900ms transform 动画、`will-change` 与 reduced-motion 行为，不再让高频更新中的 Lucide SVG stroke spinner 参与重绘。
- 同一 `activityStartSeq` 的活动摘要采用单调 `live -> archived` 展示生命周期。停止期间一旦归档，迟到的 active snapshot 不能把它重新投影为“正在操作”；后续新活动通过新的 start sequence 建立新 identity。
- 回归验收覆盖隐藏段展示投影、CSS spinner 契约、活动摘要在 `running -> cancelled -> stale running` 序列中不复活，以及既有 Activity 披露/详情交互；聚焦 46 项 Web 测试和 Web 生产构建通过。该改动不新增 I/O、定时器、缓存或历史扫描，每次稳定合并仅作常数级 lifecycle 判断，渲染范围保持在当前活动行。

### 2026-08-25：ACP 处理圆环无条件旋转

- [x] 根因与方案：共享 `AcpProcessingSpinner` 原先通过 `motion-safe:animate-spin + motion-reduce:animate-none` 响应系统动态效果偏好；Windows 关闭动画效果时 WebView2 会投影 `prefers-reduced-motion: reduce`，导致“思考中”和“处理中”圆环同时静止。按桌面产品运行反馈要求，破坏式删除该降级分支，统一使用 Tailwind `animate-spin`，不增加设置项、兼容层或第二套 spinner。
- [ ] 回归与验收：组件契约测试固定 `animate-spin` 始终存在且禁止重新引入 `motion-safe` / `motion-reduce`；执行相关 Web 单元测试、TypeScript 生产构建，并在正常与 reduced-motion 两种媒体条件下验证 computed animation。
- 性能与过度设计评审：继续使用单个 900ms `transform` CSS 动画和既有 `will-change` 合成提示，不新增 React 状态、计时器、订阅、I/O、缓存或依赖，渲染范围不变；仅在系统 reduced-motion 环境中增加两个小圆环的持续合成工作，与明确的常驻运行反馈需求匹配。

---

## 2026-08-14：完成节点转换中断恢复

- 根因修复：provider 回调已经把 durable execution 推进到 `finalizing-artifact`，orchestrator 随后却用旧的 `starting-node` 内存快照覆盖并尝试非法转换。节点边界持久化现按相同 execution identity 合并更高 durable revision，provider 返回后也刷新权威 phase；生命周期转换错误改为结构化返回，不再以 panic 终止后台推进。正常完成继续自动执行现有 control decision。
- 恢复契约：Workflow/AUTO 在 `process-interrupted / runtime-abnormal` 且当前节点为 `completed/success` 时投影独立的 `recover-completed-attempt` 能力。composer 在原“继续工作流”位置显示“恢复工作流”；点击后不重跑当前 provider，直接消费既有验证结果并进入下一节点或 `$end`。未完成 attempt 保持 `continue-current-attempt`；manual check、repair、验证失败和 ErrorBlocked 语义不变。
- 并发与幂等：恢复 command 必须携带 execution revision，并在 attempt 状态短锁内校验完整 locator、完成结果和 manual-check 边界，再复用 per-run continue lease claim 新 execution generation。双击、迟到 revision 或已推进状态均拒绝重复执行。
- 性能与复杂度：复用现有 workflow decision、attempt lock、lease 和状态模型，不新增抽象层、依赖、轮询、缓存或 repair checkpoint。节点边界仅增加常数次小型 JSON I/O，锁不跨 provider turn；数据规模和渲染范围不变，无需额外 benchmark。
- 回归验收：核心单测固定 durable phase 单调合并、completed recovery 到 `$end` 不调用 provider、重复/过期请求拒绝；桌面 VM 测试固定两种互斥 continue kind；Web 生产构建与 composer 实际交互验证固定恢复按钮文案、位置和命令路由。

---

## 2026-08-14：多工作空间 Runtime 启动恢复

- 根因修复：桌面启动恢复原先只调用 `DesktopContext.repo_root` 绑定的 `App`，把旧 Workbench 的单 workspace 启动状态误当成会话 Runtime 的恢复范围；当最近会话位于其他 `conversationWorkspaces` 时，磁盘上的 `running run + completed/success node` 不会收敛为可恢复暂停态。启动恢复现以 `conversationWorkspaces` 为唯一范围，按规范路径去重并逐 workspace 构造共享 lifecycle bus 的 scoped App。
- 状态边界：删除 `SettingsConfig.desktop_workspace`，settings schema v6 一次性移除历史 `desktopWorkspace`。`DesktopContext.repo_root` 只由启动 cwd 决定并继续服务内部配置、诊断和旧 Workbench 进程内上下文；旧 Workbench 最近列表仍独立保留，不再写入“当前 workspace”。`lastConversationWorkspace` 只用于交互排序，不决定 Runtime 恢复范围。
- 局部失败与恢复：不存在、空路径和重复 workspace 直接跳过；单 workspace 状态损坏记录 `runtime.workspace-recovery-failed` 后继续扫描其他 workspace。列表为空时 no-op，不回退扫描桌面上下文，避免 Runtime 背着用户扩张恢复范围。
- 性能与过度设计：启动时执行一次 `O(W + Σ(tasks + runs))` 的本地目录和小 JSON 扫描，无网络、provider 调用、轮询、缓存、队列或常驻任务；复用现有 `App::with_repo_root`、canonical path 和 `recover_interrupted_running_sessions`，不增加新的 workspace registry 或并发模型。
- 回归固化：桌面状态层接口测试覆盖非 DesktopContext workspace 的完成节点恢复、规范路径去重、单 workspace 失败隔离，以及空 `conversationWorkspaces` 不回退；配置迁移测试固定历史 `desktopWorkspace` 从 v5 settings 中删除，旧 Workbench 最近列表测试固定其独立职责。

---

## 2026-08-14：可扩展主题包基础能力与内置主题收敛

- 根因修复：删除把具体色板与明暗混为 `desktopTheme` 的生产模型，引入版本化 `AppearancePreference`，以稳定 `themeId × colorScheme` 表达外观，并按主题隔离视觉质量偏好。settings schema v5 一次性迁移旧四配色，不保留双写或 localStorage 旁路。
- 契约与运行时：新增封闭 Zod Theme Contract、Rust `serde` 镜像、内置 Catalog Map、resolver 和 root variable 应用器；Gold Band 与技术中性共用同一路径。后端保存接口从编译 Catalog 校验 schema/theme ID、按能力清理质量偏好并返回 canonical VM，不再维护主题 ID 白名单。
- 声明式包工具链：Gold Band 与技术中性分别位于独立 `themes/*` 包；Theme SDK 使用 DTCG + Style Dictionary 解析 alias，使用 JSON Schema + Ajv 校验 manifest/runtime contract，并生成包级 runtime JSON、recipe CSS、asset manifest、Web Catalog 与 Rust Catalog。
- 组件边界：Shell、标题栏、侧栏、shadcn Card/Input/Button/Dialog/Sheet/Popover、prompt-kit Composer 与右侧工作区通过稳定 theme role 消费材质变量，业务组件无具体主题 ID 分支。
- 材质模型：Theme Contract 保留组件 `flat / subtle / elevated` 层级以及 `solid / frosted / liquid` 封闭类型，作为通用主题 SDK 能力；当前两个内置包均声明 `solid`，不为已删除主题保留运行时入口。
- 主题视觉边界：session 切换列表统一消费 `popover` role，Gold Band 与技术中性使用包级实底表面；设置页保持当前主题摘要，完整主题包列表仅在 shadcn Sheet 中展示。
- 基础契约复验修正：Theme Contract 补齐 content header、会话消息/Composer/Activity/权限卡、工作区 Tab/资源头/文件树/编辑器和 Diff 三态的成对语义 token；技术中性主题恢复迁移前成功色、危险色和深色主按钮前景。
- 个性化权威模型：settings schema v7 引入 `PersonalizationPreference`，将 UI / 编辑器字体与字号、Agent / 个人头像图片与形状分别保存为 `theme/local/custom/user` 来源。旧字体字号字段与头像仓库选择一次性迁移；头像仓库只保留资产历史，恢复操作不删除资源。
- 内置主题收敛：删除 `themes/glass` 与 `themes/neo-brutalist` 的源包和 dist，重新生成的 Web/Rust Catalog 只允许 `builtin.gold-band` 与 `builtin.tech-neutral`。设置页不保留旧卡片、隐藏入口、fallback 包或主题 ID 特判。
- 状态收敛：现有前端 resolver 继续作为外观权威投影边界，遇到退役或未知 `themeId` 时回到 Gold Band，并同步删除无对应能力的 `visualQualityByTheme` 项；不新增 settings schema、双写或专用迁移分支。
- 性能与过度设计：Catalog 从四包缩减为两包，构建与启动期静态数据、生成 CSS 和设置页卡片数量同步下降；运行时仍为固定数量根属性与 CSS variable 写入，无新增 I/O、状态订阅、缓存、队列或渲染分支。
## 2026-08-15：会话 Markdown 行内标签对比度收敛

- 根因：反引号行内代码与本地文件标签已经拥有正确的字体和标签结构，但组件在主题 `muted` 之上再次使用 45%–50% 透明度，与会话背景二次混合后边界接近消失；问题来自现有设计实现不完整，不是各主题色板都需要单独加深。
- 实现：继续复用 prompt-kit/Streamdown copy-in 渲染器与现有 Theme SDK 语义 token，将两类行内标签统一改为直接消费 `surfaceHigh`；保留正文前景、圆角、间距和文件链接 hover 语义，不修改全局 `muted`，避免连带增强辅助区、禁用态和表格表面。
- 验收：组件单元测试固定行内代码与本地文件标签不再消费透明 `muted`；分别验证 Gold Band 与技术中性主题的 light/dark 计算样式和视觉对比度，并执行 TypeScript、Web 生产构建与 diff 检查。
- 性能与过度设计评审：仅替换两个静态 utility class，不新增 DOM、React state、Context、依赖、运行时颜色计算或主题特判；主题切换仍只触发 CSS 重算，不扩大 Markdown 渲染范围，无明显性能风险，也没有新增抽象。

## 2026-08-15：内置主题默认字重收敛为 variable 330

- 根因：两个内置主题原先共享 MiSans 静态字体家族，但正文直接使用 Regular 字形，长会话与高密度 UI 在浅色背景上形成偏黑、偏重的连续文本块；直接换成静态 Light 又会落到字体自身约 250 的字形而明显过细。问题是静态档位无法表达 Light 与 Regular 之间的目标，不应使用透明度、阴影或局部颜色补丁模拟中间字重。
- 实现：以同版本、完整中文覆盖的 MiSans variable font 替换五份静态资产，将 Tailwind `font-normal / medium / semibold / bold` 映射为字体原生轴值 330 / 380 / 450 / 520，并让 body 使用 `font-normal`。字体 face 注册范围封顶 520，使应用最高强调为真实 Semibold，不提供 Bold 630；加入 Inter Variable 后继续共用同一套连续轴语义，不新增主题 ID 分支、设置字段或 React 运行时计算。
- 验收：契约测试固定 variable 资源、330 基线、四级轴映射、body 基线和所有静态/Bold 资产退出；在两个内置主题的 light/dark 下检查正文、按钮、标题、`strong` 与行内代码的 computed font weight，并执行 Web 生产构建与 diff 检查。
- 性能与过度设计评审：单个 variable TTF 约 20.0 MB，替换的四份 300–600 静态 WOFF2 合计约 19.6 MB，安装体积基本持平并把字体请求从多个 face 收敛为一个本地资源；代价是首次字体读取集中到单文件，需在桌面 WebView 的生产页面核对加载时长。无新增 React state、Context、订阅、缓存、队列或逐元素字重计算；现有一个连续轴已经覆盖真实需求，不再保留两套字体路径。

## 2026-08-15：UI / 编辑器有序字体栈

- 根因：单个 `family` 偏好无法同时表达“英文优先 Segoe UI、中文回退 MiSans”，把完整 CSS font-family 字符串交给主题或用户又会混淆数据与序列化边界。该问题来自字体模型的根本表达能力不足，需要升级 Theme Contract 和 personalization 数据结构，而不是在中英文节点上分别硬编码字体。
- 数据与接口：Theme SDK / manifest / runtime package 升级到 v2，排版预设统一为 `{ families, fallback, size }`；personalization schema v2 使用 `fontStack: theme | custom { families }`，settings schema v8 破坏式删除 v1 单字体字段并恢复主题栈。前后端统一限制 1–16 个 family、128 Unicode 字符、大小写不敏感去重及 CSS 分隔符拒绝，保存接口返回 canonical 有序结果。
- 交互与实现：设置页复用 shadcn `Popover + Command` 搜索多选，点击顺序即优先级；已选项支持再次点击取消、删除、上移和下移，清空后恢复主题。CSS 栈由统一序列化器生成，自定义 families 后继续追加主题变量作为兜底；浏览器原生 glyph fallback 负责中英文混排，不做逐字符 JavaScript 检测。
- 主题资源包：Gold Band 与技术中性均以 `Inter Variable → Gold Band MiSans` 作为 UI 主路径，后接 MiSans、常见系统 CJK 字体与 `sans-serif`；Inter Variable 通过 Fontsource 随应用分发，不依赖系统安装，编辑器栈为 `JetBrains Mono → SFMono-Regular → Consolas → monospace`。生成 Catalog、CSS、JSON Schema 和包级 dist 同步更新。
- 验收：Theme SDK 构建测试覆盖有序栈与重复 family 拒绝；Rust 配置迁移和保存接口测试覆盖顺序保留、空栈、重复、非法字符与超长 family；Vitest 固定规范化、追加/取消/重排、空栈恢复主题和 shadcn 组件契约，并执行 TypeScript、生产构建、Rust workspace check 与内置浏览器深链验收。
- 性能与过度设计评审：每套栈上限 16 项，偏好变化仅规范化一个小数组并写一次根 CSS variable；系统字体仍只枚举一次，浏览器完成字形回退。未增加拖拽依赖、逐字符扫描、缓存、Context、队列或大范围 React 订阅，运行时复杂度与数据规模匹配，无需额外 benchmark。

## 2026-08-15：混排字体协调与完整字体目录

- 根因：Segoe UI 与 MiSans 仅共享 CSS 字重值，光学重量、字面比例和行框指标并不协调，拉丁字符与中文连续出现时仍有明显拼接感；字体选择器则把“系统字体目录”错误复用了最多 16 项的用户字体栈规范化函数，导致描述计数来自完整目录而渲染选项被截断。前者是默认主题资源不完整，后者是两个不同领域的数据边界混淆，均不采用组件局部补丁。
- 数据与实现：新增无数量上限的字体目录规范化函数，仅做 trim、大小写不敏感去重和 locale 排序；Settings 与浏览器字体探测统一消费该目录函数，用户偏好仍由原有有界、保序规范化函数约束。两个内置主题改为随包分发的 `Inter Variable → Gold Band MiSans`，设置页也允许选择两套内置字体；继续复用 shadcn `Popover + Command`，不增加逐字符 JS 分流。
- 接口回归：单元测试固定 100 项字体目录不被截断、自然排序与大小写去重，继续固定用户字体栈最多 16 项；Theme SDK 测试固定两个内置主题的 Inter / MiSans 顺序，生产构建必须包含 Fontsource variable font 资源。
- 性能与过度设计评审：系统字体仅在设置页载入时对约百项数组执行一次 O(n) 去重与 O(n log n) 排序；99 项 Command 不需要虚拟化，没有新增状态、Context、缓存、队列或额外字体枚举。Fontsource 依靠 unicode-range 按需加载字集，正文仍由浏览器原生 fallback 完成；新增依赖只承担成熟字体资源的版本与打包管理，与实际跨平台一致性需求匹配。

## 2026-08-16：Variable Font 与浮层宿主/动画边界修复

- 根因修复：Theme Contract v2 的单值 face weight 让 variable WOFF2 退化为离散 400/500/600/700 注册，破坏既有 330/380/450/520 轴映射；Dialog/Sheet/AlertDialog 缺少可验收的专用 Portal host；Dropdown/Context Menu 的 Radix 定位节点又同时承担 transform 动画与裁剪。三项均在共享契约和 shadcn primitive 层修复，不增加页面或主题 ID 特判。
- 实现：字体 face 改用有序且受资产 metadata 约束的 `weightMin/weightMax`，内置 Inter/MiSans 分别生成连续 range；应用壳增加无 transform/filter/contain/overflow 裁剪的 body-level overlay host；菜单 Content/SubContent 将材质、overflow 与 slide/zoom/fade 下沉到内部视觉层，Radix 节点只负责定位、焦点和 dismiss。
- 回归与文档：Theme SDK、Web Schema、内置主题、生成产物、SDK README、UI 交互规则、产品设置规范和主题 v2 计划同步；定向测试固定区间验证、跨主题字体身份、Portal host 和定位/视觉层职责边界。
- 性能与过度设计评审：每主题 font-face 从 8 条收敛为 2 条，只新增一个静态 host 和打开菜单时的一层 DOM；无新依赖、状态、订阅、缓存、队列、测量或 I/O，减少字体匹配键并避免定位节点的 transform 合成竞争。

## 2026-08-17：用户壁纸导入、最近使用与会话表面覆盖

- 根因与边界：Theme Contract v2 已有 `app / conversation / workspace / settings` 四类 wallpaper surface，但只表达主题包资产，不能表达用户级覆盖；会话运行页和 ACP 根容器又以实色背景遮住了 surface。实现升级既有 personalization，而不把用户资产写入主题包，也不在页面局部拼接 background image。
- 数据契约：personalization schema v4 使用 `wallpaper.byColorScheme.light / dark = { image, opacityPercent }`；settings schema v10 将 v3 单壁纸同值复制到两种模式后删除旧结构。壁纸仓库 v1 只维护全局共享的最多 10 条 MRU 资产记录，VM 不再投影冗余 selected ID，两种模式的当前选择均以 personalization 为唯一权威；MRU 裁剪保护两种模式仍在引用的资产并淘汰最旧未引用项，恢复主题不删除历史。
- 资源链路：导入支持 PNG/JPEG/WebP，限制 32 MiB、4096×4096 与 1600 万像素；blocking pool 规范化完整图至约 4 MiB，以通过容量约束的最终像素图作为单一事实源，分别编码完整图和 320×180 WebP 缩略图，不再为缩略图回读解码完整图。前端 Theme Runtime 按 URL 统一管理有界的壁纸资源状态，surface 只持有 descriptor key 投影；重复刷新直接 no-op，已 ready 资源在新 surface 首帧前同步复用，真实 URL 变更则成功后原子替换。自定义协议只接受单段 `{uuid}.full / {uuid}.thumbnail` token，并校验 UUID、索引、固定文件名和 MIME，修复 Windows `convertFileSrc` 对内嵌斜杠编码后协议解析失败的问题。
- 交互实现：设置项位于字体与头像之间，复用 shadcn Tabs、Popover、Slider、Button 与 Dialog。Tabs 首次定位当前 resolved 模式，浅色/深色的导入、选择最近、恢复和可见度分别写入对应配置。主预览限制为 256×144 的小卡片，最近列表全局共享且只懒加载缩略图。
- 可见度与会话：浅色/深色各自保存 20%–100%、默认 60%、步长 1% 的可见度。拖动只更新局部 state 与当前 active scheme 的 CSS variable，commit 才持久化。运行时在主题或系统明暗变化后从 resolved scheme 重新推导用户壁纸；会话 surface 与 Composer 透明承载边界保持不变。
- 接口与回归：Rust 测试覆盖 v9 迁移、导入/最近 10 条/恢复、缺失资产收敛、单段协议 token、完整图与缩略图的可解码及尺寸契约、路径穿越与损坏索引拒绝；Web 测试覆盖最近选择、1% 规范化、Windows URL、设置顺序、小卡片/Dialog、Slider commit 和会话 surface 投影；Theme SDK 测试固定图片/scrim 分层。
- 本轮验收：settings v10 迁移与桌面端壁纸校验、共享 MRU/去重上限及跨模式选中资产保留、单侧缺失收敛共 4 项 Rust 定向测试通过；Theme Engine、壁纸偏好、主题运行时与 surface 首帧协调 4 个 Vitest 文件共 33 项通过，覆盖同 URL 跨页面复用、不同 surface 主题壁纸首次加载后复用、URL 切换原子替换、失败隔离、迟到回调和首次绘制前协调；TypeScript `--noEmit` 与 Web 生产构建通过。内置浏览器 deep link 实测浅色/深色独立选择与 60%/59% 可见度切换、共享 2 条最近记录、单侧恢复、1% 键盘步进、720px/1440px 无横向溢出，以及会话 surface 壁纸生效；本轮复核快速对话与设置页分别挂载 `conversation / settings` surface、切换无控制台错误。Composer 卡片保持实色，其内容轨道、左右 padding 与整宽 sticky footer 计算背景均为透明。
- 性能与过度设计评审：最近历史严格有界为 10，当前只加载一张完整图，Popover 懒加载缩略图；不新增 ResizeObserver、窗口尺寸状态、无界缓存、队列或逐帧持久化。壁纸资源表仅保留当前主题有效槽与两种明暗选中资产，surface 使用 `WeakMap` 随 DOM 生命周期自动释放，不进入 React 根状态或扩大页面重渲染范围。拖动热路径只做常数次 CSS variable 更新，会话 wallpaper surface 只增加固定伪元素；复用已有 Theme Engine、协议、shadcn 组件和 blocking helper。对 2188×1272、3.51 MiB 真实资产的 release 分段测量确认，删除为生成缩略图而进行的完整图二次解码可减少约 80–100 ms 的 O(像素数) 重复工作；最终像素图与编码字节共存的峰值与旧链路回读后的共存模式同阶。本轮不新增依赖、通用缓存框架、并发队列或过渡动画，与实际规模匹配。

## 2026-08-17：新会话启动态与内容加载遮罩分域

- 会话运行页继续复用 runtime canonical lifecycle 展示 `preparing-workspace / starting-node` 启动态；当前 attempt 的 ACP session 尚未 ready 时不再叠加“正在加载会话”的历史内容遮罩，也不允许 partial session 继续渲染“无 session id”、空 timeline 或 composer。`initializing` 统一提前返回现有品牌 Logo 启动态，ready 后原子开放完整会话。既有 established session 未命中正文缓存时仍保留原加载遮罩，缓存、readiness fetch 和请求数量均不变。
- Web 接口级回归覆盖新会话 launch、已建立未缓存会话、hydration 完成和 `initializing -> isolated loading surface` 四类边界，防止后续再次把启动生命周期与历史内容加载或 partial session shell 混合。
- 过度设计与性能评审：仅从 leaf 已有 `current + sessionEstablished` 事实派生展示条件，不新增状态、状态机、缓存、轮询、请求或额外订阅；判断为 O(1)，不会扩大渲染或 I/O 范围。

## 2026-08-17：隐藏 Prompt 链接与右侧只读工作区

- 根因与交互：隐藏 system/runtime context 的解析与紧凑投影设计继续保留，但消息组件内的 `Collapsible` 把长文档误建模为局部披露内容，未复用已经支持多 Tab、Markdown/源码和只读查看的右侧工作区。隐藏段改为带 Lucide 文档图标、原有语义颜色和字符数的 shadcn link Button；删除块背景、箭头、内联展开正文与 content-expansion 生命周期。
- 数据与接口：新增 `HiddenPromptSectionWorkspaceLocator = AcpAttemptWorkspaceLocator + eventId + eventSeq + partIndex` 和对应资源类型/稳定 key。点击经既有 `openResource` 打开或激活 Tab；资源 LRU 只保存 locator。内容面板按 `eventSeq - 1` 请求一个 ACP 语义块，再以精确 event identity 和 part index 解析正文，不使用标题或显示文案反查，也不把长 prompt 复制进全局工作区状态。
- 组件复用：内容区继续复用 `SystemPromptPanel`；产品层 rendered 模式使用现有 prompt-kit `Markdown / Streamdown` 生成静态 Markdown DOM，raw 模式使用现有 `WorkspaceFileEditor(editable=false)` 展示源码，两种视图二选一挂载并沿用既有视图偏好。右侧 Tab renderer、图标和中英文缺失态只扩展一个资源分支，不新增后端接口、编辑器、Markdown renderer、依赖、缓存或持久字段。
- 渲染回归修正：用户截图确认最初实现把 `rendered` 映射成了 CodeMirror `live-preview`，虽隐藏部分 Markdown 标记但仍是编辑器排版，并非真实渲染。共享 `SystemPromptPanel` 现统一承担只读文档模式映射，渲染态使用 Streamdown、源码态使用 CodeMirror，工具栏明确切换两种产品语义；DOM 回归固定默认标题/粗体真实渲染、源码原文不变、`editable=false` 及双向切换。
- 验收：Vitest 固定隐藏段图标链接、不生成 Collapsible、点击输出稳定 part index、跨 branch/section key 隔离、精确 event revision 解析，以及工作区只发起一次单语义块读取并把目标正文交给共享只读面板；同时执行完整 Web 测试、TypeScript、生产构建，并在内置浏览器 deep link 下检查浅色/深色、hover/focus、窄工作区和长 Markdown 的 rendered/source 切换。
- 性能与过度设计评审：未打开时不挂载 Markdown/编辑器或解析第二份正文；打开时为一次有界语义块 I/O 与一次 O(prompt length) 解析。移除隐藏正文 `<pre>`、展开 state、token 和展开宽度测量，工作区命令继续使用低频稳定 Context，Tab/宽度变化不扩大历史 Markdown 渲染范围。一个 locator 资源类型足以表达真实生命周期，不增加状态机、队列、缓存或假设性抽象，无需专项 benchmark。

## 2026-08-17：快速对话跨页面工作空间恢复

- 根因：快速对话已有 `draftConversationWorkspaceId` 作为应用运行期事实，设置页返回时能正常消费它；但从会话详情点击“快速对话”时，导航决策无条件优先取当前 run 的 workspace，将仍然有效的 draft 投影覆盖；这是既有状态转换优先级错误，不是持久化模型缺失。
- 实现：纯导航决策固定“quick-chat draft → 当前会话 workspace → 最近会话 workspace”优先级，并继续在快速对话入口的用户事件链中应用；某 workspace 下显式点击“新会话”才切换 draft，无 draft 时的会话与最近工作空间兜底不变。
- 边界与回归：Vitest 契约覆盖从设置返回保留 draft、从其他 workspace 会话详情返回仍保留 draft、无 draft 时使用当前会话 workspace、非会话页无 draft 时回退最近会话 workspace，并固定入口接入统一决策。`lastConversationWorkspace`、会话列表最近 workspace 置顶和后端状态结构不变，单纯切换快速对话 workspace 不触发排序。
- 本轮验收：已完成导航决策、回归契约与文档静态复核；按用户要求未运行 Vitest、TypeScript、Web 生产构建或页面交互验证。
- 性能与过度设计评审：决策为固定三项的 O(1) 分支，只在用户点击导航时执行；不新增 effect、状态、持久字段、I/O、依赖、缓存、队列或渲染订阅，现有 App 级 draft 已足以表达跨页面生命周期，无需升级为跨重启偏好。

## 2026-08-17：已发起 ACP 会话动态配置目录

- 根因与数据边界：此前把 Run 发起时的不可变配置快照和 ACP Provider 的可变能力目录绑定在一起，导致 Doctor 已发现新模型、权限或 select config option 后，历史 session 仍只能看到旧目录。修复后 Run 初始绑定继续不可变；session override 继续按 attempt 持久化；可选目录改为 Doctor / Session 最近成功观测的投影，不新增独立 catalog aggregate。
- 权威与时间规则：Session 通过 `session/new / resume / load` 持久化目录及 `configCatalogObservedAt`，Doctor 通过既有 Agent registry 提供同一 Provider parser 的目录。Doctor 仅在严格更新时覆盖展示和选择校验，同时间或更晚 Session 优先；失败 Doctor、空 capabilities 和无关 Agent 更新不覆盖当前 session 的最近成功目录或当前值。
- 惰性确认：选择 Doctor-only 配置时持久化 override 与 `configCatalogRefreshRequiredAt`；下次 continue 复用现有 attached-session registry、singleflight 和 resume/load 链路，对原 session 强制重载一次。Session 响应目录先落盘再应用 override；仍不支持时返回 `acp.session-config-value-unavailable` 的 Config / Manual 结构化错误并阻止 prompt，不静默回退默认模型。前端补拉最新 Session，将失效 override 保留为禁用项，用户改选后继续。
- 前端状态边界：会话页只从当前 session provider 派生 Doctor catalog，并与低频 session config view model 合并；Doctor current value 不进入业务 session。配置选择继续使用 optimistic patch，但以单调 mutation generation 约束失败回滚，早期失败不覆盖后续选择或期间到达的新目录；结构化不可用错误显示统一 i18n 文案。
- 性能影响：每次 Agent registry 或当前 session config 变化只对当前 Provider 的小型目录做一次 O(catalog) 投影，签名不包含纯时间戳，流式消息不会触发配置栏重渲染。只有用户实际选择 Doctor-only 值后的下一次 continue 增加一次 resume/load；不扫描历史 attempt、不批量改写 session、不增加轮询、无界缓存、队列或扩大锁范围，无明显性能风险，无需专项 benchmark。
- 过度设计评审：现有 Run snapshot、session metadata、Agent registry、attached runtime registry 和 resume/load 已足以表达全部不变量；仅增加两个 session metadata 时间字段与结构化错误，不新增 catalog 服务、后台同步器或跨层 identity，复杂度与低频竞态风险匹配。
- 回归要求：Rust 接口测试固定 Doctor 严格更新时写 refresh marker、Session 同时间优先、attached session 只触发一次 reload，以及失效配置归类为 Config / Manual 并携带可用值；Web 测试固定 Doctor/Session 投影、current value 所有权、失效项禁用和无关 Agent 更新不改变配置签名。合入前执行桌面 crate check、Web 生产构建，并用前端 deep link 验证正常与窄宽度选择器。
- 本轮验收：4 个 Rust 定向接口测试通过；ACP session config 与错误 i18n 共 26 个 Web 测试通过；`cargo check -p gold-band-desktop` 与 Web 生产构建通过。内置浏览器 deep link 实测模型复合目录、权限目录和相邻菜单切换；720×900 下文档 `scrollWidth === clientWidth`、两个配置触发器与 352px 菜单均未越界，控制台无 error/warn。预览数据不含历史 session，Doctor/Session 新旧目录和 stale override 由上述接口测试验收。

## 2026-08-17：PostTurn finalize 边界继续并发送

- 根因：`artifact-emission.json(finalizing)` 原本只表达“上一业务 turn 已完成”，provider 却把它解释为任何 Runtime continue 都必须直接恢复 finalize；因此 `UserMessage` 类型的继续并发送也会被隐藏 artifact prompt 覆盖。修复扩展既有 checkpoint phase，不建立第二套 Runtime 状态机。
- 生命周期：`finalizing + RuntimeResume` 继续重新请求完整 artifact；`finalizing + UserMessage` 在发送用户 prompt 前原子切换为 `business-turn`，先执行新的业务 turn，成功后再回写 `finalizing` 并生成新的隐藏 finalize。业务 turn 再次中断时保留 `business-turn`，后续继续不得直接跳 artifact。
- 提示契约：继续并发送使用独立中英文条件模板，并直接消费现有 `OutputEmissionMode`。三个分支都先执行本消息中的用户指令，再继续完成此前任务；`PostTurnProjection` 本 turn 不适用此前的 artifact 输出约束且不输出 artifact，任务完成后再独立归一化；`InlineControl` 在任务完成后再于同一 turn 按当前契约输出 artifact；无 contract 时不提 artifact。纯继续模板保持原语义。
- 回归验收：Provider 单元接口 25 项通过，覆盖纯继续、继续并发送、二次停止恢复和损坏 checkpoint；Runtime 继续组合 prompt 定向测试 1 项通过；PostTurn 发射模式与中断完成判定定向测试 4 项通过。`git diff --check` 无空白错误。
- 本轮增量回归：中英文条件模板覆盖 PostTurn、InlineControl 与无 contract 三个分支；固定 workflow continue 接口固定 PostTurn 组合 prompt；AI-DYNAMIC emission 映射固定 bootstrap=InlineControl、worker/acceptance=PostTurn、merge=无 contract；AI-DYNAMIC 集成测试目标完成编译。
- 性能与过度设计评审：继续复用 attempt 级单个小型 checkpoint、既有原子 JSON 写入与 canonical `OutputEmissionMode`，只增加一次 O(1) phase/模板分支；动态 leaf 在既有 graph 读取与锁区间内取得目标节点 emission policy，不增加 graph 加载、timeline 扫描、缓存、队列、锁、依赖或渲染订阅。仅在 finalize 边界插入新用户业务 turn 时多写一次 `business-turn`。新增 durable phase 用于表达“新业务 turn 尚未可靠完成”这一现有 `finalizing` 无法表达的具体不变量；提示分支不新增状态或第二套策略事实源，复杂度与恢复正确性风险匹配。

## 2026-08-17：快速会话上下文选择器选中态统一

- 根因：工作空间 `SelectTrigger` 和工作位置 `Button` 是同级上下文控件，但两者复用了不同 primitive 默认值：交互态没有统一，且 `SelectTrigger` 的 `data-[size=default]:h-9` 以更高选择器优先级覆盖业务层 `h-7`，造成静态背景与高度不一致。问题属于共享视觉契约缺失，不是选择状态或持久化缺失。
- 实现：继续复用现有 shadcn `SelectTrigger`、`Button` 与主题 token；两个专用上下文触发器的 surface 只由共享交互 class 管理，工作位置按钮不再叠加通用 `button-ghost` 主题 recipe。静态态统一透明，hover / focus / menu open 统一使用 `accent / accent-foreground`，并显式把两者收敛为 28px 高、相同圆角和水平内边距。工作位置菜单复用工作空间已有的指针/键盘关闭分流：指针关闭阻止 Radix 回灌焦点并 blur，键盘关闭保留 focus restoration。定时任务胶囊变体、工作位置校验和偏好作用域不变。
- 回归要求：现有 jsdom 组件接口测试固定两个触发器静态透明、交互态 accent、Select size variant 与 Button 高度/内边距，并固定指针关闭工作位置菜单后触发器不重新获得焦点；同时执行 Web 类型检查、生产构建，并在内置浏览器 deep link 下用 computed style 检查静态、hover、菜单展开/外部关闭、浅色/深色和窄宽度表现。
- 性能与过度设计评审：只增加常量级 class 合并与 DOM 属性，不新增 state、effect、持久字段、依赖、I/O、缓存、队列、订阅或额外渲染；两个现有控件和一个共享样式常量足以表达不变量，不引入新组件或通用状态抽象，无需专项 benchmark。

## 2026-08-17：超长粘贴转临时文本附件与附件独立提交

- 根因与契约：原提交资格只检查正文，附件虽已具备完整选择、物化和 provider content block 链路，却不能独立构成用户输入；首次实现又把“超长粘贴优化”错误扩大成正文总长度规则。修复将会话输入统一定义为 `正文非空 || 附件非空`，并把自动转附件严格限定为单次 paste 事件；完全空 payload 仍在前后端拒绝，不增加占位正文或超长文本专用后端类型。
- 前端实现：快速对话与 ACP composer 复用附件 hook 的 paste handler。优先沿用既有文件粘贴行为；没有文件且本次 `text/plain` 超过 6,400 字符时，阻止默认插入并生成一个可见的普通 `text/plain;charset=utf-8` 草稿附件。输入框已有正文保持不变，普通键盘输入、程序化草稿恢复和发送阶段不再做长度转换。生成附件继续使用既有数量、总大小、File 物化和草稿恢复机制，未提交时不创建本地文件。
- 后端与生命周期：附件物化目录迁移到系统 `%TEMP%/gold-band/conversation-attachments/<uuid>/`。初始会话仅在附件存在时允许空 requirement，并通过 conversation 专用 task 创建入口保留通用 task 的正文必填约束；ACP command、Direct 队列和 Runtime Continue 均按完整 payload 校验，附件-only 用户消息继续携带可见消息语义和普通附件 metadata。
- 回归固化：已补充 Web paste 阈值与两个 composer 接线测试，以及 Rust command、队列、Runtime Continue 和初始 task 创建接口测试，覆盖 6,400 边界、6,401 字符生成附件、普通输入/提交不转换、附件-only 放行与完全空 payload 拒绝。此前本任务的 Web 定向用例 49/49、纯附件消息 DOM 用例 3/3、`cargo test --lib session_prompt_` 5/5 均通过，`cargo check --lib` 通过；按用户要求，本次提交阶段不再执行编译、测试、构建、浏览器或 EXE 验证。
- 性能与过度设计评审：普通输入不再执行任何长度门禁；仅 paste 事件读取一次剪贴板纯文本并做长度判断，超过阈值后才分配一个文本 File。生成文件继续受既有附件数量、总大小上限和后端路径校验约束。没有新增依赖、持久字段、缓存、轮询、队列、并发机制或额外渲染订阅；复用现有附件 aggregate 与物化接口足以表达生命周期，无需新的状态机或专项 benchmark。

## 2026-08-18：ACP 附件按实时能力投影与纯附件消息展示

- 根因与契约：附件已经具备统一解析与持久化模型，但出站 block 若不消费 live ACP capability，就会把 provider 差异硬编码到 Agent 名称或强制所有 Agent 支持同一种内联内容；同时消息组件把空正文和空消息混为一谈，使合法的附件-only 输入产生空气泡或被过滤。修复继续复用现有附件 aggregate、ACP `initialize` capability 与 timeline 用户事件，不新增 provider 特例或第二套消息模型。
- 数据与接口：当前物理连接成功 `initialize` 返回的 `agentCapabilities` 是附件投影的唯一事实源。图片能力存在时发送 `image`，可嵌入上下文能力存在时发送 `resource`，对应能力缺失、畸形或为 false 时统一降级为协议基线 `resource_link`，并保留 URI、名称和 MIME metadata；不按 Agent 名称、版本或 Doctor 历史结果猜能力。初始 task 输入与本轮 user input 继续维持既有归属，只在 content block 投影边界选择表现形态。
- 消息展示：附件-only 的 optimistic 与 canonical 事件携带同一附件 metadata；正文为空时不渲染空用户气泡，但附件行仍完整展示并与头像圆形区域垂直居中。正文与附件并存、图片与文件分行及右侧工作区预览契约保持不变。
- 回归固化：Rust 接口测试覆盖 capability 完整、缺失、畸形和显式关闭时的 `image / resource / resource_link` 分支及 link metadata 保留；Web 组件测试覆盖 optimistic/canonical 附件-only 消息不出现空正文气泡、附件仍可见且布局对齐。上述用例已包含在此前通过的本任务定向验证中；按用户要求，本次提交阶段不再运行编译或测试。
- 性能与过度设计评审：每个附件仅在既有线性解析过程中执行常量级 capability 分支，整体保持 O(附件数)；复用连接级 capability cache，不新增请求、扫描、持久字段、缓存、队列、锁或渲染订阅。现有 canonical identity 与生命周期足以表达全部不变量，无需新 aggregate、状态机或专项 benchmark。

## 2026-08-17：会话附件统一在右侧工作区预览

- 根因与契约：右侧工作区与 `draft-attachment` / `conversation-asset` 资源模型已经成立，但 composer 点击入口按 `image/* + previewUrl` 特判，文本回退旧 Dialog；消息附件则已走工作区，形成同一附件领域的消费路径分裂。本次删除类型分流，快速对话、ACP composer 和消息气泡都只提交工作区资源 locator，不增加第二套预览状态或兼容入口。
- 内容读取：桌面选择器为已选择且不超过附件单文件上限的文本文件签发精确路径、revision 绑定、短期有效的只读内容 URL；草稿 Tab 激活后才读取正文。浏览器 `File` 直接在 Tab 内按需读取。图片继续使用既有 revision-bound 预览 URL，消息附件继续使用 canonical `task-inputs` / `user-inputs` 读取接口；任一路径都不在附件列表或 composer 状态中保存正文。
- 组件复用：抽取共享只读文本工作区查看器，普通文本沿用 CodeMirror 只读源码视图；`.md/.markdown` 直接复用 `ReadonlyMarkdownWorkspaceViewer`，默认实时渲染并可切换只读源码。草稿附件面板与消息附件面板共用该组件，图片继续复用 `WorkspaceImageCanvas`，不引入新的编辑器、Markdown renderer 或基础 UI 控件。
- 图片缩略图：消息气泡下的图片缩略图删除底部文件名覆盖条，避免与既有 Tooltip 重复展示并遮挡图片；hover/focus 继续通过 shadcn Tooltip 显示“文件名 + 大小”，按钮保留完整 `aria-label`。普通文件 chip 与 composer 附件样式不变。
- 回归与验收：接口测试固定所有 composer 附件映射为稳定 `draft-attachment` 资源；组件测试覆盖桌面文本按需读取、图片不触发文本读取、Markdown 路由到共享双模式查看器，以及消息气泡文本附件进入同一查看器；Rust 测试固定临时内容 URL 的 MIME、正文与 revision-bound 协议。执行 Web 定向测试、完整 Web 测试、类型检查、生产构建与相关 Rust 测试，并用内置浏览器 deep link 验证两个 composer 入口和已发送消息入口。
- 性能与过度设计评审：正文读取发生在用户打开活动 Tab 后，单次只读取一个受附件大小上限约束的文件；不新增全量扫描、N+1、轮询、持久字段、缓存、队列或宽 Context 订阅。共享查看器是两个既有面板的最小复用边界，现有 workspace identity 和 Markdown 模式已经能表达全部不变量，无需新增 aggregate、状态机或专项 benchmark。

## 2026-08-17：快速对话工作空间信息栏明暗层级一致

- 根因与实现：工作空间信息栏原先在浅色主题使用 `surface-high`，深色主题却使用与页面相同的 `conversation-background`，导致深色下主体、顶部圆角和两侧连接肩一起融入背景。信息栏现统一消费主题的 `surface-high` 语义材质，主体与连接肩继续共享一个 CSS 变量，不增加 `dark:` 特判或硬编码颜色。
- 回归要求：组件与布局契约固定信息栏不再消费 `conversation-background`，并在浅色、深色主题中检查主体、圆角和两侧连接肩均可辨；工作空间/工作位置控件的透明静态态及 hover、focus、menu open 交互态保持不变。
- 性能与过度设计评审：仅替换一个静态 CSS 变量映射，不新增状态、effect、依赖、I/O、缓存、订阅或渲染分支；主题切换仍只触发样式重算，性能影响可忽略，现有布局常量足以表达该视觉契约。

## 2026-08-18：快速对话与会话详情 Composer 视觉基线归一

- 追溯结果：快速对话顶部工作空间信息栏的深色 surface 消失由 `da174e0`（`feat(conversation): add worktree-backed quick chats`，2026-08-17 15:31:30 +08:00）引入；该提交首次创建信息栏时把深色背景映射到与页面相同的 `conversation-background`。修复继续使用前述统一 `surface-high` 契约。
- 工具栏归一：保留快速对话 Direct / Workflow / Auto 的容器查询布局和会话详情停止 / 继续 / 队列的业务差异，只共享视觉尺寸基线。快速对话附件入口收敛为 28px，模型、权限和发送保持 32px；正文与底栏之间删除额外分割线并缩小留白，配置触发器与会话详情复用同一静态尺寸常量。
- 输入高度：删除 prompt-kit 中唯一的 `userResizable` 分支、指针监听和用户最小高度状态；快速对话与会话详情统一为内容自动增长到 320px 上限，之后仅 textarea 内部滚动，不展示浏览器原生 resize 角标。定时任务配置等独立表单 textarea 的手工调整能力不受影响。
- 回归要求：接口测试固定两类 composer 的配置控件尺寸、快速对话无顶部分割线和 28px 附件入口，并固定会话详情 textarea 为 `resize-none`；autosize 测试继续覆盖未达上限隐藏滚动条、达到上限封顶和内部滚动。浅色、深色及窄宽度页面验证不得出现布局退化。
- 性能与过度设计评审：删除一次 pointerdown 后的全局 pointerup / pointercancel 监听和局部最小高度状态；正常输入仍保持每次受控值提交一次布局测量，不增加 state、effect、请求、缓存、订阅或渲染分支。两类 composer 的业务结构不同，因此不强行合并组件；只共享已有布局层的尺寸常量，复杂度低于原实现且无新增性能风险。

## 2026-08-18：定时创建 Composer 工作空间样式归一

- 根因与实现：定时创建与普通快速对话已经共享 prompt-kit composer，却在工作空间入口上分别渲染底栏胶囊和顶部信息栏，造成同一输入组件的视觉分叉。现删除定时模式底栏胶囊，让两种创建模式复用现有 80% 宽圆角梯形信息栏与 shadcn workspace 选择器。
- 能力边界：普通快速对话继续展示 workspace 与工作位置；定时创建只展示 workspace，固定使用主工作区，不展示新工作树，也不向定时定义或调度运行时增加 `workLocation`。后续若开放定时工作树，必须另行定义 occurrence/run 级 identity 与准备生命周期，不能从当前 UI 偏好推断。
- 回归要求：组件接口测试固定共享信息栏外壳和定时模式无工作位置触发器；源码契约固定定时提交不携带 `workLocation`、底栏不再条件渲染第二个 workspace 控件。执行 Web 定向测试、生产构建，并用内置浏览器 deep link 验证 `/chat` 与 `/chat/scheduled-tasks/new` 的输入框外观一致、定时模式无工作树入口且无横向溢出。
- 性能与过度设计评审：复用现有组件并减少一处重复控件，不新增 state、effect、请求、持久字段、缓存、队列、订阅、扫描或后端 I/O；现有数据模型足以满足当前范围，无需提前扩张调度定义和工作树生命周期。

## 2026-08-18：ACP artifact 按可信终态回扫最近三条消息

- 根因与契约：旧候选模型分别累计全部 identified/anonymous 文本，并在缺少可信终态边界时倒序寻找合法 JSON；当正常稳定输出之后以无 ID 错误文本结束时，它可能拾取前面的非 artifact JSON，再由 repair 生成合法 JSON，错误地把未完成业务收敛为成功。现改为复用 canonical timeline 的稳定 message identity 与流边界，只维护当前 turn 最近最多 3 条 Agent message；是否允许回扫由最终消息身份决定，不再扫描拼接输出或无界历史。
- 终态矩阵：最后一条有稳定 ID 时，从最后一条开始向前检查最近最多 3 条消息，提取第一个可解析 JSON 后进入 schema 校验；全 turn 都无稳定 ID 时只能校验最后一条无 ID message，非法则进入 hidden invalid-output repair；turn 内出现过稳定 ID、但最终 message 无 ID 时返回 `provider.acp-terminal-message-unidentified + Manual recovery`，直接进入可继续的 `Paused + RuntimeAbnormal`，不回扫、不发送 repair。Direct/普通对话仍展示全部可见文本，不应用 artifact 终态异常。
- Repair 生命周期：允许进入 validator 的非法输出最多自动 repair 三次。三次耗尽不把 run 终结为 Failure，也不使用不可继续的 `ErrorBlocked`；当前 node/run 持久化为 `Paused + RuntimeAbnormal`，清除已结束的 active runtime execution ID、保留 attempt 与 ACP continue identity，composer 开放普通输入，用户可补充指令并继续当前 attempt。
- 回归要求：ACP accumulator 单测固定 stable→anonymous、stable message identity 切换、全 anonymous 分块及三条窗口上限；Provider 单元测试固定最终稳定消息、向前命中更早 JSON（含无 ID 消息）、第四条以外不命中、全 anonymous 合法 JSON 和 mixed terminal Manual error；Runtime 接口测试固定 mixed terminal 只调用一次 Provider 且不发 repair，以及三次 repair 耗尽后的可恢复状态；Conversation lifecycle/Web composer 测试继续固定 `continue-current-attempt`、`acp-prompt` 和输入可用。
- 性能与过度设计评审：复用 canonical message identity、既有 JSON/schema validator、repair 计数和 RuntimeAbnormal 生命周期；以最近 3 条、单条 64,000 字符的有界窗口、一个 active identity 和一个“曾出现稳定消息”布尔事实替换旧的多路累计与无边界候选语义，不新增持久字段、依赖、队列、缓存或状态机。处理保持 O(事件数)，单次候选扫描固定 O(3)，总内存继续受固定上限约束，无需专项 benchmark。

## 2026-08-18：Direct 排队转直接发送的边界收敛

- 根因：Direct composer 提交时根据当时 lifecycle 走 `queue-prompt`，但上一 turn 可能在命令到达后端前已结束。后端按最新权威状态直接发送并返回 `acp-session` 是合法结果，前端却只接受 `queued`，因而在消息已执行且已回答后误报发送失败、恢复已提交草稿，存在重复发送风险。
- 实现：保留后端现有队列与直接发送决策；前端排队提交分支将 `queued / acp-session` 统一判定为已接受。`acp-session` 回执继续走既有 session/lifecycle 合并，成功后释放附件且不恢复草稿；未知或拒绝结果仍保持失败保护。
- 回归要求：Web 接口映射单测固定 `queued` 和 `acp-session` 两种合法结果，并保持 `rejected` 及其他命令结果进入失败路径；执行定向 Web 测试、类型检查、生产构建和内置浏览器 deep link 交互验收。
- 验收结果：定向 Web 测试 43 项通过，生产类型检查与 Vite 构建通过。内置浏览器 deep link 到 `run-053` Direct 队列夹具，固定“UI 显示加入队列、browser API 返回 acp-session”的真实边界；正常宽度、760px、恢复 1440px 和长文本提交均清空草稿且不显示发送失败，页面无横向溢出。完整 Web 回归 1460 项中 1459 项通过；唯一失败是既有 `TurnFileChangesCard` 源码 `mb-3` 与旧测试仍断言 `mb-2` 的无关基线差异，本次不修改该布局契约。
- 性能与过度设计评审：只增加常量级返回类型判定并复用现有 session 合并入口，不新增状态机、持久字段、依赖、请求、缓存、队列、扫描、宽订阅或渲染边界；复用现有 canonical lifecycle 与结果联合类型即可表达全部不变量，无需新 aggregate 或专项 benchmark。

## 2026-08-18：快速对话与会话详情输入首行基线统一

- 根因：两类 composer 已经复用 prompt-kit `PromptInputTextarea`，但会话详情保留 textarea 自身的 `py-2`，快速对话却把垂直留白全部移到父 surface 并覆盖为 `py-0`；带命令标签时 prompt-kit wrapper 又同时承担横向和垂直 inset。共享组件设计成立，问题来自盒模型职责分叉，不是字体栈、业务状态或 autosize 生命周期缺失。
- 实现：布局层新增共享 textarea 基线，快速对话与会话详情统一消费 `min-h-12 + py-2 + text-sm/leading-6`；快速对话 surface 收敛为 `px-4 py-2` 并继续保留正文 `px-0`，会话详情继续保留 `px-3`。prompt-kit adornment wrapper 删除垂直 padding，只保留横向 inset，命令标签与 textarea 的 `py-2` 首行对齐。删除快速对话旧 `min-h-14/py-0` 消费路径，不增加兼容分支或局部位移。
- 回归要求：接口与 DOM 测试固定两类 composer 使用共享 textarea class、快速对话横向边缘不变、普通和命令标签状态都由 textarea 单独持有 `py-2`，wrapper 不再重复垂直 inset；autosize 继续覆盖 320px 上限与内部滚动。执行 Web 定向测试、类型检查、生产构建，并在实际快速对话和会话详情中检查空态光标、中文 placeholder、输入正文、命令标签、窄宽度与长文本。
- 性能与过度设计评审：仅合并静态 Tailwind class 与现有 prompt-kit wrapper 职责，不新增 state、effect、ResizeObserver、请求、持久字段、依赖、缓存、队列或渲染订阅；autosize 的测量次数和 O(文本高度) 浏览器布局成本不变。现有布局常量与共享 copy-in 组件足以表达不变量，无需新组件、字体系统改造、自绘光标或专项 benchmark。

## 2026-08-18：附件内联上下文预算与图片派生图

- 根因：附件 resolver 在 Agent capability 投影前完整读取并展开所有受支持文件，文本可把几十 MiB 正文直接放入上下文，图片也只受上传大小约束；长粘贴另有 6400 字符常量，形成三套不一致边界。问题来自 prompt attachment 缺少统一 projection policy，不是某个 Agent 或扩展名特例。
- 数据与配置：`configs/app-config.toml` 新增 `conversationInlineContentMaxBytes=64000`、`conversationInlineImageMaxBytes=4194304`、`conversationInlineImageMaxDimension=2560`，经 `ProjectAppConfig -> RuntimeConfig -> AppConfigVm / WorkerInvocation` 显式传递。粘贴与文本按 UTF-8 字节消费同一内容边界；图片字节与像素尺寸单独管理，避免用文本 token 预算误伤视觉输入。图片默认值参考主流视觉模型的 2048–2576 px 高细节区间，并为桌面截图保留 4 MiB 派生图预算。
- 实现：`AcpContentBlock` 增加不可重新展开的显式 `ResourceLink`。超限文本 metadata-first 直接生成 link；图片先读 metadata/header，超限时直接从文件流进入 Rust `image` 的受限解码器，依次尝试 2560 px 内的无损 WebP、JPEG 92 和有界缩小尺寸，只保留本轮内存派生图，失败回退原文件 link。只有原始编码和尺寸都在预算内时才读取原图字节。user input 继续原子持久化到 attempt，但复制改为流式 I/O，不为大文本分配整文件缓冲。live ACP capability 仍决定预算内 `Image / Resource` 是否可发送，link-only Agent 行为不变。
- 回归要求：Rust 固定配置 roundtrip/override/VM、文本 64000/64001 字节、图片字节与尺寸压缩、损坏图片 link fallback、显式 link 在完整 capability 下仍不展开、Task/Attempt 原文件归属；Web 固定 ASCII 与中文多字节粘贴边界。执行 root/desktop 定向单测、类型检查、生产构建和会话页 deep link 粘贴验证。
- 验收结果：`cargo check -p gold-band`、`cargo check -p gold-band-desktop`、Rust provider 32 项、config 41 项、显式 link 与桌面配置 VM 定向测试、前端粘贴 2 项、TypeScript 检查、格式检查和生产构建均通过。内置浏览器实际验证 64000 ASCII 保留正文、64001 转附件、21333 个中文字符（63999 字节）保留正文、21334 个转附件；普通图片附件可加入 composer，800px 窄宽度与恢复 1440px 后输入框和附件入口持续可见，控制台无 error/warn。
- 性能与过度设计评审：大文本从整文件读取改为一次 metadata，用户附件复制使用 O(1) 内存流式 I/O；超限图片不保留原始编码缓冲，解码最长边限制为 8192、分配限制为 128 MiB、缩放编码最多 10 次，且不进入 React 热路径。只新增一个相关 policy 值对象和一种协议意图，不新增持久状态、缓存、队列、轮询、全量扫描或并发机制；现有 attachment identity 与 ACP capability 足以表达不变量。

## 2026-08-18：用户级有界 Runtime 恢复索引与启动性能回归修复

- 根因修复：0.13.0 为恢复全部会话工作空间，把启动恢复从单 workspace 扩张为 `conversationWorkspaces × tasks × runs` 全历史扫描；其成本随用户历史累计，且与新增 scheduler 启动串行叠加。恢复范围设计本身错误，不通过并行扫描、缓存或延迟整页刷新掩盖。
- 数据边界：新增用户级、跨工作空间、不可按缓存或 workspace 删除的 `core.db`；当前 recovery schema v2 仅包含有界 `runtime_recovery_candidates`，并已从 v1 一次性删除 `workspace_key`。`run.json` 仍是唯一 lifecycle canonical state；候选表只保存可能非终态的 locator 和 execution fencing token，进程内 `ActiveRuntimeRegistry` 只投影本进程真实活跃 run，不建立第二套生命周期。
- 一致性顺序：run 写 `Running` 前必须先登记候选并把 token 写入 `run.json.execution.recoveryCandidateToken`；首次启动、显式继续、动态继续和重试统一在 drive 首次成功持久化 `Running` 后才提交 provisional registration，execution 已被取代时撤销候选，避免前置失败进入 active registry。持久化为 Paused/Completed 后按 `(project_id, task, run, token)` 条件删除。崩溃最多多留候选，不会漏掉已 Running run；旧 generation 的迟到清理不能删除新 generation。候选上限 4096，满额拒绝新 run，不淘汰旧候选。
- 启停恢复：Tauri setup 先开放窗口壳，在 blocking worker 中只读候选并以 workspace identity 与 canonical run 校验；不存在、已非 Running 或 `recoveryCandidateToken` 与候选 token 不一致的 run 只条件消费旧候选，不改动 canonical run，只有 Running 且 token 一致时才收敛为 `Paused + ProcessInterrupted` 后消费。成功候选用完即删，下一次启动不再看到；恢复 lifecycle 通过既有局部 event 更新。scheduler 以恢复完成为启动门闩；退出先关闭 admission、等待 scheduler 停止，再仅暂停 registry 快照中的活跃 locator。
- 失败隔离：SQLite 使用 WAL、FULL synchronous、短事务和 3 秒 busy timeout；registry 锁内不做 SQLite、文件、provider、scheduler I/O，也不跨 await。候选全集无法读取时维持全局恢复门闩；单 workspace 或单候选恢复/删除失败只隔离对应 workspace，其它工作空间继续，避免为跨 workspace 强一致引入全局长锁或死锁。
- 进程边界：用户级 `core.db` 与桌面 Runtime 统一为同用户、同发布渠道单实例。复用 Tauri 官方 single-instance plugin 作为第一个 plugin 建立进程互斥；第二次启动只恢复并聚焦已有主窗口，不进入 setup、恢复或 scheduler，不新增自研 lease、heartbeat 或 PID fencing。
- 验收要求：接口测试固定跨 workspace 候选恢复、不扫描无候选历史、stale token 不能删除新 generation、已 Paused 候选只消费且下一次启动 no-op、候选登记失败不进入 active registry、未成功持久化 Running 的 provisional registration 会撤销、正常退出暂停并清理后候选表为空，以及 scheduler 只在恢复 gate 后注册。另以三个依次销毁和重建的 `DesktopState` 固定完整启动边界：第一个实例写入隔离的物理 `core.db`，第二个实例通过生产恢复入口消费并删除，第三个实例确认候选为零且 run 未被重复改写。执行 core/desktop 定向测试、两个 Rust crate check、格式与 diff 检查，并以 production build 对候选为空和跨 workspace 候选场景复测 Windows 启动耗时。
- 性能与过度设计评审：启动复杂度从无界 `O(W + Σ(tasks + runs))` 收敛为 `O(C)`，`C <= 4096` 且正常只等于可能非终态 run 数量；空候选只有一次 SQLite 小查询。只新增一张索引表、一个进程级 coordinator 和一个 token metadata，不新增轮询、历史缓存、无界队列、全量 UI refresh 或跨文件事务，复杂度与防漏恢复及 generation 竞态相匹配。
# 2026-08-19：修复 ACP 连续发送生命周期竞态

- 根因：prompt admission 引入后，迟到的上一轮 metadata 可能与新 turn 的 `starting` header 合并，执行器检查失败却静默 `Ok(())`，导致第二条 Direct 消息没有 ACP RPC 且 composer 长期禁用。该问题属于 lifecycle identity fencing 未实现完整，不通过前端解锁按钮规避。
- 实现：新增 `turnId + lifecycleOperationId + acpRevision` 的原子 execution claim；claim 是不可变 `AcpLifecycleOwner` 的唯一创建边界，并把它贯穿到 provider runtime。`acpRevision` 定义为 ownership generation，只在 admission、claim、stop 接管或 terminal 时推进；provider 的 `Accepted/Running/terminal` lifecycle 写入及外层失败结算全部用 runtime 原始 owner 做精确 CAS，不再从当前 snapshot 反推执行身份。stop 接管后由持有新 control-plane owner 的停止控制方在 cancel dispatch 后结算 `cancelled`，不等待已失权的 provider runtime 回写。取消、完成、revision 推进或新 turn 接管时，旧 owner 写入均为 stale no-op，不升级为执行失败。
- 回归：Rust 接口测试覆盖 claim 单次消费、空/错误身份拒绝、同一 owner 的 running 写入保留 generation、owned terminal 推进 generation 且旧 owner 不可复用、stop 接管后旧 provider running/terminal 写入不得覆盖 `CancelRequested`、stop owner 可以结算 `cancelled`，以及 execution failure 只能结算拥有者。ACP events 92 项测试、desktop crate check、Rust 格式与 diff 检查均通过。
- 性能与过度设计评审：复用现有 attempt 文件锁、revision 和 active registry；新增操作均为 O(1) JSON header 条件写入，不扫描 timeline、不增加缓存/队列/依赖，也不扩大 provider 网络调用锁范围。

## 2026-08-21：本轮文件变更按工具成功终态结算

- 根因：turn 文件聚合此前把 ACP `content[type=diff]` 直接视为已发生变更；Codex 在授权前会先以 `in_progress` 发布候选 diff，用户拒绝后同一工具以 `failed` 收敛，但候选版本仍被固化成 `fileChangeSet`。这是 diff 证据与工具生命周期契约缺失，不是前端卡片过滤问题。
- 数据与接口：中间 diff 继续写入既有 BLAKE3 CAS 与 mutation journal，供工具卡片预览；运行时只为本 turn 出现 diff 的 `branchId + toolCallId` 维护终态投影。`TurnFileStore::finalize_turn_branch` 必须显式接收结构化工具终态，只有 `completed/success/succeeded` 纳入，`failed/error/cancelled/canceled` 及无终态调用排除。permission 的选择、取消和文案不进入该接口，不形成第二套文件事实源。
- 历史收敛：change set schema 升级为 v4；读取旧 schema 时只在迁移慢路径扫描该 branch canonical timeline，并按同一工具终态重建。没有成功终态的旧集合写回 v4 空集合，前端既有 `fileCount === 0` 契约会移除错误卡片；正常 v4 读取与新 turn 结算不扫描 timeline。
- 回归与验收：核心接口测试覆盖 `in_progress(diff) → completed(无重复 diff)` 生成变更、缺失成功终态不生成、`in_progress(diff) → failed(重复 diff)` 不生成、终态映射与 permission 无关，以及 schema v3 失败工具集合迁移为空。定向 `cargo test --lib acp::turn_files` 17 项通过；宽泛 package test 被既有 `tests/entity_uuid_test.rs` 缺少 `NodeState.acp_storage_schema_version` 的夹具编译错误阻塞，未在本需求中修改该无关用户改动。
- 性能与过度设计评审：新 turn 每个 diff 工具只增加一次 `HashMap` 常数级记录，状态随 turn 结算清空；聚合仍只遍历该 turn 已有的有界 mutation，不新增依赖、持久字段、队列、缓存、锁或普通路径 timeline 扫描。历史全 timeline 读取只发生在 schema v1-v3 的一次性迁移慢路径。现有 `toolCallId`、timeline status 与 change-set 模型足以表达不变量，无需 permission→diff 关联、新 aggregate 或第二套状态机。

## 2026-08-21：Direct 首轮停止后的空会话投影

- 根因：后端为避免把只有 `initialize`/outbound raw frame、尚未完成 `session/new` 的占位数据误报为真实 ACP session，正确过滤了 `unavailable + no sessionId + empty Timeline` 的 Provider session；前端只实现了 Workflow/AUTO 的“初始化被中断”投影，却没有按 Direct attempt lifecycle 建立可继续对话的空壳，最终把合法的 `paused + cancelled` 状态降级为通用“ACP 会话失败”。
- 实现：保留后端 Provider session 物化边界；仅当当前 Direct attempt 同时满足 `paused + process-interrupted`、runtime inactive、ACP `unavailable + idle + cancelled`、非 stopping 且 composer 为 `normal + acp-prompt + input unlocked` 时，前端从 canonical lifecycle 投影无 sessionId 的空 Timeline shell，并复用现有 prompt-kit composer。该状态停止无意义的 session 查询；下一次发送返回 active lifecycle 后恢复既有查询与 `initialize -> session/new -> session/prompt` 路径。Workflow/AUTO 与真实失败投影不变。
- 回归与验收：纯策略测试固定 Direct 正向条件及 orchestrated、active、established、failed、无 submit target 等反例；DOM 接口测试以 `session=null` 挂载早停 Direct attempt，固定不显示 ACP failure/Workflow continue、composer 可输入且发送调用携带当前 attempt locator 与无 sessionId shell。定向 Web 51 项测试、TypeScript 检查与生产 Vite 构建通过；内置浏览器使用同构的 Direct 早停夹具 deep link 验证空 Timeline、普通 composer、无错误页、无 Workflow continue、发送前按钮可用，点击后草稿清空且无 console error/warn。验收后已删除临时夹具并关闭页面与开发服务。
- 性能与过度设计评审：复用既有 attempt lifecycle、session shell 和 composer，不新增持久字段、状态机、依赖、缓存、队列或兼容层；判断与渲染均为 O(1)，早停空会话还会停止最长约 30 秒的 missing-session 轮询。现有 canonical lifecycle 已足够表达不变量，无需恢复旧 placeholder session 或建立 Direct 专用数据模型。

## 2026-08-21：Git 前置条件对话框接入右侧源码管理

- 根因与实现：快速会话选择新工作树遇到缺失首次提交等 Git 前置条件时，对话框仍提供外部 Git 下载页，却没有接入已经存在的右侧源码管理恢复入口。现复用 shadcn Dialog/Button 与 `RightWorkspaceCommands`，按当前会话 `projectId + scopeKey` 打开或激活主工作区源码管理 Tab。真实窄窗口验收同时发现显式 open revision 会在 auto-collapse 判定紧凑布局前被提前消费，导致只创建隐藏 Tab；修复后的 transition 仅在紧凑状态就绪时消费 revision 并展开 Sheet，不新增页面、Git 状态或旁路导航。
- 交互契约：恢复动作按“取消 / 重新检测 / 使用主工作区 / 打开源码管理”排列；前两项业务恢复动作使用 outline，“打开源码管理”作为唯一主按钮，删除对话框内“打开 Git 下载页面”。源码管理工作区仍可按 capability 提供对应安装或仓库操作，完成后由用户显式重新检测。
- 回归要求：组件接口测试固定中文按钮顺序、下载入口缺失、主次按钮 variant，并在真实 `RightWorkspaceProvider` 中点击主按钮，断言对话框关闭且 active Tab 收敛为当前项目的 `source-control:<projectId>:main`；布局 transition 测试固定非紧凑中间态不提前消费 open revision，紧凑态收敛后自动展开 Sheet。执行定向 Web 测试、TypeScript 检查、生产构建，并用内置浏览器 deep link 验证正常与窄宽度下的对话框及右侧 Tab 切换。
- 验收结果：Git 对话框、低频工作区命令和 auto-collapse 定向回归 28 项通过，TypeScript 检查与生产 Vite 构建通过。内置浏览器 deep link `/chat` 验证 1280px 下按钮顺序与主按钮黑色样式正确、点击后直接打开源码管理 Dock；520px 下按钮纵向同序且无溢出、点击后直接打开源码管理 Sheet；页面无 console error/warn。Web 全量 1546 项中 1544 项通过：ACP 重入计时用例单独重跑通过；唯一稳定失败是与本需求无关的 `ConversationPromptQueue` 既有组件仍使用共享 `border` class、测试却断言 `border-0` 的基线不一致。
- 性能与过度设计评审：点击只提交一次 O(1) 工作区资源 locator 与局部导航状态；源码管理内容继续在活动 Tab 中按需加载。不新增依赖、持久字段、缓存、队列、扫描、轮询、宽 Context 订阅或重复 Git 检测；现有 workspace identity 与低频命令 Context 已足够表达不变量，无需新 aggregate、状态机或专项 benchmark。

## 2026-08-21：Windows 任务栏图标 DPI 清晰度契约

- 根因：Windows ICO 已包含 `16 / 24 / 32 / 48 / 64 / 256` 多档图层，但 16px 位于目录首层。Tauri 2 在 Windows 的 `generate_context!()` 中只解码 ICO 首层作为实时窗口图标；Windows 任务栏在实时窗口图标、EXE 多分辨率资源和 Shell 缓存之间选择来源时，可能把该 16px 位图放大，因此同一开发进程或生产安装版在不同 DPI、固定和激活状态下清晰度不一致。
- 实现：继续复用 Tauri 官方多分辨率 ICO 与既有品牌矢量源，只把 32px 调整为首层，保留 16/24/48/64/256 图层和 32 位 RGBA，不增加运行时 `set_icon`、任务栏特判、Explorer 缓存清理或新图标生成依赖。开发窗口与生产 EXE 继续消费同一 `src-tauri/icons/icon.ico`。
- 回归要求：品牌资源测试直接解析 ICO directory，固定图层顺序为 `32 / 16 / 24 / 48 / 64 / 256`、全部 32 位，并继续逐层检查无白色 matte 像素；执行定向 Vitest、Web 生产构建、Tauri debug 构建资源检查和 Windows 原生任务栏验证。
- 性能与过度设计评审：改动仅调整 6 个 ICO directory entry 的顺序，不改变像素数据或文件规模；运行时仍只加载一个固定尺寸窗口图标，不新增状态、依赖、I/O、缓存、队列、锁、渲染或后台任务，对启动和运行性能无可测影响。现有资源契约足以表达问题，无需新增图标服务或平台状态机。

## 2026-08-21：超长用户消息折叠

- [x] 用户消息正文超过 240px 时默认折叠，在气泡正文下方使用现有 shadcn `Button` 提供“查看更多 / 收起”；短消息不显示入口，中英文文案同步维护。
- [x] 折叠状态保存在单条消息组件，不修改 canonical Timeline、event window、分页 cursor、分页 anchor 或偏好存储；展开/收起复用既有 chat content-expansion controller，保留自动贴底意图，并尊重展开期间的用户主动滚动。
- [x] DOM 接口回归覆盖短消息、超长消息默认折叠、展开/收起与 controller token 配对；同时回归自动贴底与 ACP 分页测试，执行前端类型检查、生产构建及会话 deep-link 长文本验证。
- 性能与过度设计评审：每条用户消息只增加一个局部 `ResizeObserver` 和两个布尔展示状态，测量为 O(1) 高度比较且仅在结果变化时提交状态；不扫描 timeline、不重解析 Markdown、不增加网络 I/O、依赖、缓存、队列、并发或持久字段。现有 prompt-kit/shadcn 组件和滚动 controller 已足以表达不变量，无需新消息模型或第二套滚动状态机。

## 2026-08-21 ACP lifecycle consistency follow-up

- [x] Unified metadata patching under the existing attempt lock and made `acp.snapshot.json` the only production write target; runtime control, catalog/config updates, stop cleanup, and provider metadata preserve the latest lifecycle owner and revision. Legacy `acp.session.json` is read-only fallback that seeds the first canonical snapshot write for old attempts, with no continued dual write.
- [x] Centralized the owner terminal fallback at `client::run_prompt()` for Direct、Workflow、AUTO and hidden finalize/repair, and propagated canonical `prompt_accepted` failures through desktop production wiring. Startup/setup/callback or durable prompt-queue failures therefore cannot leave a claimed turn permanently active or make finalize fail with a false session-busy state.
- [x] Added generation-scoped artifact finalize identities with durable checkpointing. Recovery reuses a checkpointed generation until its matching control turn is canonical terminal, then creates a fresh generation so an already-settled prompt identity is never resubmitted.
- [x] Made lifecycle ViewModel reads non-mutating and removed normal query dependence on complete `acp.raw.jsonl` recovery. Frontend lifecycle merge now rejects stale revisions, re-derives projection fields, and uses project/outer/branch-complete cache locators.

Scope deliberately excludes Timeline index decomposition, a new database/state machine, and speculative optimization of low-frequency background paths. The acceptance target is functional convergence after stop/continue/follow-up and bounded I/O on normal conversation reads.

Final audit kept provider-owned catalog fields (`models/modes/configOptions/observedAt`) replaceable while protecting command-owned overrides and refresh markers. The terminal fallback now belongs to the shared ACP client prompt boundary instead of Direct-specific outer settlement, preventing preparation/provider/join failures from diverging by execution mode. Round-detail optimistic state includes the effective branch in the same complete locator as session/event caches. These are constant-size metadata/key operations and remove redundant legacy writes; no new scan, cache, queue, lock, dependency, or benchmark is introduced.

## 2026-08-21：ACP terminal 展示与 Workflow 节点跟随

- [x] Composer 只在 ACP live turn active 时使用 Timeline 最新 `thought/tool/textDelta` 细化当前状态；terminal 后历史 `textDelta` 不再维持“回复生成中”，Workflow 仍处理时改由 Runtime phase 显示中性处理状态。
- [x] 普通 Workflow/AUTO attempt VM 携带 `run.execution.revision` 水位，同时继续以完整 locator 限定 `current/active/phase`；Direct 无该 revision，AI-DYNAMIC leaf 使用自身 execution。
- [x] 普通节点 durable `NodeStarted` 通过完整 project/session locator 触发局部 Run 刷新和 RuntimeControlled auto-follow；manual/NonRuntimeControlled 不抢焦点，Dynamic 内部 metrics leaf 不映射。canonical Run 边界在合并刷新 pending/in-flight 期间优先于迟到 ACP 更新；`NodeCompleted` 保持 repair/metrics 原顺序，不作为详情刷新入口。
- [x] 自动回归与编译：Composer、session follow/navigation、sidebar event DTO 共 123 项通过；Rust Run event mapping 与 lifecycle VM revision 定向接口测试各 1 项通过；Web TypeScript/生产构建、`gold-band` 与 `gold-band-desktop` 编译检查、Rust 格式与 diff 检查通过。
- 性能与过度设计评审：所有新增判断和事件映射均为 O(1)，复用单 in-flight 合并刷新；无 Timeline/raw 扫描、轮询、缓存、持久字段、队列、依赖或新状态机。现有 ACP lifecycle、Run revision 和完整 locator 足够表达问题，不为低频边界或剩余单体 index 成本扩建设计。

## 2026-08-21：首轮 ACP 模型覆盖值持久化边界

- [x] 修复首轮 provider 元数据写回时的配置覆盖丢失：admission 快照尚未有 `sessionId` 时保留本次执行显式的 `modelOverride`、`permissionModeOverride` 与 `configOptionOverrides`；已建立 session 后继续以用户命令写入的覆盖字段为准，字段被清除时迟到 provider 快照不能复活旧值。
- [x] Rust 接口级回归覆盖“首次写回保留 `sonnet/high`”与“已建立 session 的显式清空保持为空”两条路径；前端继续只显示 Gold Band override，不从 Agent `currentValue` 反推用户选择。
- 性能与过度设计评审：复用现有 attempt metadata 锁、lifecycle owner 和 override 字段，仅增加常量级存在性判断与字段合并；不新增状态机、持久字段、RPC、扫描、缓存或队列，正常路径 I/O 与复杂度不变。

The final desktop regression audit also fixed a V7 index contract gap: canonical Agent launches carrying only `goldBandConversation.launchedAgentExecutionId` were grouped into an activity summary, so pagination and Agent links diverged after re-entry. V8 recognizes the canonical identity as both an indexed launch and a standalone semantic block; older indexes rebuild once, while steady-state page and runtime reads retain the same bounded behavior. Desktop fixtures now declare the current attempt storage schema instead of weakening production validation.

## 2026-08-21：子 Agent execution 跟随父 prompt turn 终止

- 根因：Agent index 用当前 session 是否 active 推导全部历史 execution。Turn 1 取消后，Turn 2 把同一 session 重新置为 running，Turn 1 中没有结果证据的旧 Agent 因而被重新投影为 running；异步 launch tool 的 completed 回执还可能在 session 正常结束时被误当作子 Agent 完成。
- 数据与接口：复用 root timeline index 已有的 Gold Band prompt locator、`startedSeq / endedSeq / status / endedAt` 与 Agent launch sequence，建立可删除重建的父 turn 边界投影，不增加子 Agent canonical 状态机。顶层 launch 绑定最近的前序 prompt；嵌套 launch 继承父 execution 的 turn。prompt terminal sequence 覆盖 launch sequence，或已有下一 prompt 时，该 turn 对此 execution 已终止；只有 branch result 证据保留 completed，否则统一 interrupted 并清除 attention。当前 turn 活跃时继续按 branch 事件投影 queued/running/waiting_permission。
- 身份与回归：`AgentExecutionId` 继续由 session id 与本次 launch tool id 生成；新 turn 即使 Agent 名称、描述或 prompt 相同，只要发生新的 launch 就形成新的 execution。Rust 接口回归固定“Turn 1 cancel + Turn 2 active 不复活旧 Agent”“新 turn 同描述创建独立 execution”“父 turn completed 但只有异步启动回执仍 interrupted”以及真实 Agent result 保持 completed，并要求完整重建与 materialized index 两条路径结果一致。
- 性能与过度设计评审：正常查询只在已加载的 timeline materialized index locator 上生成并排序 prompt 边界，不读取 prompt body、不扫描 `acp.raw.jsonl` 或完整 timeline，也不按 Agent 重复 I/O；Agent 与 prompt 的内存匹配为现有 attempt 小集合上的有界处理。未新增依赖、持久字段、缓存、队列、锁或独立生命周期 aggregate，现有 prompt sequence/revision 和 branch result 已足以表达不变量。

## 2026-08-22：子 Agent 前端 live 状态终态收敛

- [x] 根因：Tauri 停止完成发布的是不含 session 正文的 lifecycle patch；前端全局 conversation event router 只消费 event/session，导致子 Agent 最后一条普通事件留下的 running snapshot 跨会话切换继续覆盖后端 interrupted projection，只有应用重启清空内存后才恢复正确显示。
- [x] 状态与接口：复用现有完整 attempt locator、ACP lifecycle revision、`latestTurnStatus/liveTurnActivity` 和 `AcpSessionVm.branchExecution/timelineProjection`，在同一个有界 branch live store 中统一收敛 lifecycle-only terminal、session event 与显式 session query。terminal patch 只中断仍为 queued/running/waiting_permission 的 execution并保留既有终态；Agent execution 终态不被迟到普通事件或较旧 lifecycle revision 复活，新的父 turn 仍通过新 launch ID 建立独立 execution。
- [x] 展示与回归：Agent 外层 link 和右侧 branch 标题共享终态优先解析，权威 interrupted/completed/failed 不再被旧 live running 覆盖。接口与 DOM 回归覆盖 lifecycle-only cancel、completed 保留、attention 清理、迟到事件/revision、权威 branch query 校正以及外层/右侧一致显示；切换会话无需重启即可看到已中断。
- 性能与过度设计评审：终止 patch 只扫描现有最多 64 个 branch snapshot，复杂度 O(64)；session 校正只遍历响应已携带的直属 Agent projection，不增加 IPC、Timeline/raw 读取、轮询、持久字段、依赖、缓存、队列或状态机。现有 canonical lifecycle 和 session projection 已足以表达不变量，无需扩展后端协议。

## 2026-08-22：后台追问 terminal lifecycle 重进恢复

- [x] 根因：切出会话期间的 Timeline live event 已由有界 replay 保留，因此重进能显示 Agent 最终回复；但 lifecycle-only terminal 只发送给事件发生时已挂载的详情页，路由快照只保存派生 status/revision，不足以收敛恢复出的 `awaitingResponse + activeTurnPromptId`，导致 Composer 持续显示“回复生成中”。后端 ACP snapshot、raw RPC、Timeline 与 Workflow paused 状态均已正确落定。
- [x] 实现：复用现有 conversation event router 与最多 64 个最近活跃 branch 的内存上限，在 root branch snapshot 保留完整 lifecycle，并通过既有 `mergeConversationAttemptLifecycle()` 按 ACP、Runtime 与 prompt queue revision 分域单调合并。root 会话挂载或重进时把 retained lifecycle 投影到既有 Composer 状态机；普通 live subscription、停止后追问、Direct 与 NonRuntimeControlled follow-up 共用同一入口，只读 Agent branch 不消费 root Composer lifecycle。
- [x] 接口回归：router 测试固定 terminal lifecycle 完整保留以及较旧 running revision 不得复活 terminal；DOM 重进测试固定“后台 terminal + stale running session + sending optimistic prompt”必须展示最终正文、不显示“回复生成中”且 textarea 已解锁。
- [x] 验证：conversation router、重进恢复、Composer 状态和 Runtime continue 相邻回归共 99 项通过；Web TypeScript 检查与生产 Vite 构建通过。按用户约定不进行桌面端手工验证，由用户使用真实会话验收。
- 性能与过度设计评审：每个既有 branch snapshot 仅增加一份常量级 lifecycle 对象，Map 定位、revision 合并和挂载读取均为 O(1)；容量仍为 64，不新增后端状态、持久字段、IPC、轮询、Timeline/raw 扫描、依赖、缓存层或状态机。现有 canonical lifecycle 与有界 live snapshot 足以覆盖真实切页窗口，不为被淘汰后的低频边界扩展长期缓存。

## 2026-08-22：工作流继续动作按当前 attempt 归属

- [x] 根因：会话生命周期投影使用 Run 级 `process-interrupted` 判断 continue 类型，却没有先校验恢复命令的完整 locator 所有权，导致历史 `completed/success` attempt 也得到 `recover-completed-attempt`，显示“恢复工作流”；实际后端命令已有 current locator 校验，点击只会被拒绝。
- [x] 状态与接口：复用 `run.currentRound/currentNode/currentAttempt` 作为普通 Workflow/AUTO 的 continue owner。只有当前 attempt 可以投影 continue action：未完成断点使用 `continue-current-attempt`（“继续工作流”），已成功完成但后继边尚未提交的断点使用 `recover-completed-attempt`（“恢复工作流”）；历史 attempt 固定 `continueKind=null`。AI-DYNAMIC 保留既有复合节点语义，以当前 outer attempt 作为命令 owner，dynamic leaf 继续描述内部断点。
- [x] 回归：Rust ViewModel 接口测试固定同一 paused Run 下“历史成功 dev-test 无 continue action、当前 paused test 显示继续工作流”，并保留 dynamic parent running/paused、stale cancelled leaf 和 launching suppression 的既有覆盖；83 项 Conversation ViewModel 测试通过。
- 性能与过度设计评审：只复用已加载 Run 的三个 locator 字段增加 O(1) 身份比较，不新增协议字段、持久状态、状态机、依赖、缓存、I/O、Timeline/raw 扫描或锁范围；现有 canonical current identity 已足够表达动作所有权，无需复制 UI 状态。

## 2026-08-24：快速会话工作树身份与启动错误贯穿

- [x] 根因：会话工作树实现虽然声明使用 run identity，实际短哈希只包含规范化仓库路径与 `taskId/runId` 顺序编号；`.gold-band` 与 `.maling` 等独立数据域操作同一 Git repository 时会重复产生相同编号，继而与共享 `.git` 中的既有分支和 linked-worktree 登记冲突。该问题属于 canonical identity 作用域设计缺陷，不通过自动 prune、删除历史分支或重试补丁规避。
- [x] 数据与实现：run 创建时先生成并复用现有 `run.uuid`，与既有 `projectId/taskUuid` 一起通过项目已有 BLAKE3 生成 16 位稳定短 ID；路径继续归属当前通道受管 `worktrees/`，Git 分支继续使用 `gold-band/conversation/<shortId>`。同一 durable run 重算保持幂等，不同通道独立创建的 run 不再因显示编号相同而碰撞；不迁移或改写已失败空会话和历史 worktree。
- [x] 错误契约：Git worktree 创建边界返回 `workspace.worktree-create-failed` 结构化 `RuntimeErrorInfo`；后台准备失败复用 canonical `run_paused` 事件并写入 `controlFailure.runtimeError`，timestamp 与 durable `run.updatedAt` 对齐。会话页沿用既有 runtime error 映射按错误码显示中英文恢复文案，原始 Git diagnostic 只保留给日志和诊断，不新增对客后端文案。
- [x] 回归与验收：Rust 测试固定相同 durable identity 幂等、task/run UUID 任一变化均生成不同 worktree、Git 创建失败保留结构化错误，以及后台失败事件可由 Conversation VM 按当前 pause timestamp 读取；前端映射测试固定中英文错误码文案。`cargo test --lib worktree` 17 项、后台错误定向测试、desktop Conversation VM 定向测试、两个 crate check、前端 6 项定向测试、TypeScript 与生产构建均通过。WB/MALING 桌面实测在 Test 工作空间新建 `task-008/run-001` 的工作树 + AUTO 会话，使用独立 `task_uuid/run_uuid` 生成 `.maling/.../worktrees/439c32bbd972b134` 与 `gold-band/conversation/439c32bbd972b134`，顺利越过 workspace preparation 并以 success 完成；验收后已移除这个零独有提交的临时 worktree/branch，测试 task 已移入回收站。未迁移失败会话，未 prune、删除或改写任何既有 `.gold-band` worktree/branch。
- 性能与过度设计评审：identity 计算只哈希三个固定长度字段，错误事件只增加一个常量大小对象，均为 O(1)；不新增 Git 查询、全量扫描、迁移器、状态机、持久字段、依赖、缓存、队列、轮询、锁或渲染订阅。现有 UUID、BLAKE3、`RuntimeErrorInfo`、run event 与 i18n 已足够表达不变量，无需清理其他产品通道的 Git 资源。

## 2026-08-25：AI-DYNAMIC 后置工作区状态文案

- [x] 根因：Runtime 使用同一个 `PreparingWorkspace` canonical phase 表达初始环境准备和 AI-DYNAMIC leaf 完成后的 checkpoint、fork、release；后端 Composer 与前端合并投影都把该 phase 无条件翻译为“正在准备开发环境…”。生命周期所有权和状态转换正确，缺陷属于正确设计下的展示投影实现不完整，不拆分或复制 Runtime 状态机。
- [x] 实现：复用 dynamic graph phase、leaf `completed/success`、graph 对 causal leaf 的既有投影所有权和 leaf lifecycle revision。仅在该组合成立时，Conversation VM 输出 `processing-workspace + conversation.runtime.processingWorkspace`；初始 workspace preparation 保持原文案，Direct 和普通 Workflow 不变。前端 Composer 合并按 Runtime revision 选择权威 facet 后保留该后端展示语义，并继续让停止态优先。
- [x] 回归范围：Rust ViewModel 接口覆盖初始准备原文案、已完成 causal leaf 的详情与 session tree 新文案、并行运行 leaf 和历史 leaf 不继承新文案；前端状态测试覆盖后端新投影消费、ACP facet 合并保持和停止优先级。
- 性能与过度设计评审：所有判断均针对已加载 graph、leaf 和 lifecycle 做 O(1) 比较，不增加 I/O、Timeline/raw 扫描、React 订阅或渲染范围；不新增 aggregate、持久字段、revision、状态机、依赖、缓存、队列、并发或锁。现有 canonical lifecycle 已足够表达区别，因此只完善消费端投影。

## 2026-08-25：Release WebView 致命错误写入 runtime.log

- [x] 根因：Release WebView 已有 `window.error`、`unhandledrejection` 和 React uncaught 入口，但只针对 Maximum update depth 输出 page console；macOS WebKit 默认不把 page message 转发到系统日志，导致页面已加载但黑屏/渲染失败时 `runtime.log` 没有前端异常。该问题属于全局观测设计正确但持久化实现不完整；本次补齐统一诊断边界，不把某个用户现场特征硬编码为特例，也不假定已经定位黑屏业务根因。
- [x] 数据与接口：新增固定 `FrontendErrorReportInput`，只包含 `window-error / unhandled-rejection / react-uncaught`、错误与 component stack、脚本行列、pathname/user agent 和不含文本内容的 DOM 结构摘要。前端经 Runtime API 的单一 `report_frontend_error` command 上报，Rust 二次规范化后用现有 tracing 写 `runtime.log`；browser preview 保持 no-op。禁止输入值、聊天正文、prompt、附件路径、工具内容、Token、query 和任意对象透传。
- [x] 资源与失败边界：message 4096 字符、stack/component stack 16384 字符、其余字段 64–2048 字符，前后端独立限长；同一 message+stack 5 秒去重，10 秒最多 5 条。调用同步抛错、Promise reject 或日志失败全部静默收敛，不能形成新的 unhandled rejection、改变页面行为或覆盖 canonical Runtime 错误。
- [x] 回归与验收：前端固化三种入口、结构化字段、所有字段限长、去重/异常风暴限流、sink/context throw/reject 和既有 Maximum update depth 控制台诊断；桌面 API 固化 command/参数契约，Rust 固化 Unicode 安全二次限长和 command 接受结构化 DTO。Web 定向 2 个文件 25 项、Rust 定向 2 项通过；`npm run web:build`、`cargo check -p gold-band-desktop` 和 `cargo fmt --all -- --check` 通过，只有项目既有 dead-code 和 Vite chunk-size warnings。
- 性能与过度设计评审：正常路径仅有三个全局 listener 和一次 pointer 摘要更新，不发生 IPC、扫描或轮询；只有异常路径执行 O(受限字符串长度) 规范化与最多 5 次/10 秒 IPC，内存去重集合受相同窗口约束。复用现有 Runtime API、Tauri command 和 tracing/轮转日志，不新增依赖、持久字段、状态机、缓存层、后台队列、重试或 UI；一个有界 reporter 足以补齐观测缺口。

## 2026-08-25：runtime.log 有界异步 writer

- [x] 根因：`runtime.log` 已有 8 MiB/4 份轮转和高频事件限流，但 tracing subscriber 仍在每个调用线程同步获取 `Mutex<FileRotate>` 并执行磁盘写入；日志调用扩展到桌面 IPC、Runtime 与 ACP 后，慢磁盘、杀毒扫描或轮转可能把 best-effort 诊断反向变成业务线程延迟。该问题属于正确容量设计下线程隔离实现不完整，不通过只移动 WebView error command 的局部补丁规避。
- [x] 数据与生命周期：复用已有 `tracing-appender`，以单一 1024 行 lossy 队列连接全局 subscriber 与专用 `gold-band-runtime-log` writer 线程；`FileRotate` 只归 writer 线程所有。队列满时丢弃并累计 dropped-lines，不反压调用线程；CLI 在整个 command 作用域持有 `RuntimeLogGuard`，桌面端由 Tauri managed state 持有到进程退出，正常退出执行有界 flush。强制终止允许损失队列尾部，符合 `runtime.log` 非 canonical、best-effort 的既有契约。
- [x] 范围：只替换 `runtime.log` writer，不修改 `events.jsonl`、ACP Timeline/raw/diagnostics、session metadata、run/node/dynamic graph、配置或其他文件写入语义；8 MiB/4 份轮转、日志级别、target filter、格式和调用点保持不变。
- [x] 回归与验收：确定性门控测试固定 writer 被阻塞且队列满时调用方继续并准确计数丢弃行，异步队列测试固定 guard 释放时 flush 并保持 8 MiB/4 份轮转；Rust observability 相关 21 项、`cargo check -p gold-band-desktop`、`cargo check -p gold-band --bin gold-band -j 1`、Rust 格式与差异检查通过。首次并行验证因同时存在多个 Rust 构建导致 `rustc-LLVM out of memory`，改用 `--lib -j 1` 后通过，确认不是实现或测试失败；编译仅保留项目既有 dead-code warnings。
- 性能与过度设计评审：调用线程只承担现有事件格式化和一次有界 channel `try_send`，不再获取文件锁或执行 write/flush/rotate；常驻资源为一个 1024 行队列和一个日志线程。使用已有依赖和标准 guard，不新增自研队列、重试、持久状态、业务状态机或第二套 writer；lossy 策略避免异常洪峰把诊断压力传导到业务线程。

## 2026-08-25：右侧源码管理绑定当前会话 Worktree

- [x] 根因：源码管理资源、Store 和后端已按 `projectId + workspacePath` 支持 linked worktree 隔离，但右侧通用入口只传 `projectId`，固定生成 main 资源；AI-DYNAMIC child 会因此显示源分支主工作区，并把 `.gold-band/worktrees/...` 错列为未跟踪目录。该问题属于正确设计下入口投影实现不完整，不修改 Git 或 Runtime canonical workspace 模型。
- [x] 数据与实现：复用 `ConversationSessionLeafVm.worktreePath`、完整会话导航 locator 和现有 repository/workspace 会话 Store。右侧会话树只保留一个项目级源码管理 Tab；数据会话按 `projectId + normalized workspacePath` 隔离。主工作区保持 `null`，路径不通过分支名或展示文本反查。
- [x] 接口回归：会话导航测试固定 dynamic leaf 选择、无 attempt 路由下的 selected-session 回退、主工作区和非会话页面；入口测试固定当前 worktree 路径。后续会话树切换收敛修正在 2026-08-26 章节验收。
- 性能与过度设计评审：每次会话导航只增加常量级 locator 查找；显式 session locator 使用现有树索引遍历，最坏 O(当前 session tree)，无额外 Git I/O、全量文件扫描、轮询、缓存、队列、锁、持久字段或 Context 订阅。源码管理仍只在活动 Tab 按需加载，并继续使用既有 24 项 repository/workspace LRU；现有 identity 足以表达不变量，无需新 aggregate、状态机或依赖。

## 2026-08-26：源码管理单 Tab 跟随会话工作位置

- [x] 缺陷形成路径：前一版只让“打开源码管理”入口携带当前 `worktreePath`，同时把路径写进右侧 Tab key；右侧工作区状态却按整个 Run 保存。因此源码管理已经在 main 打开后，切换 dynamic child 只更新会话分支展示，不会更新已有源码管理 Tab。根因属于正确的 repository/workspace 会话隔离设计与错误的可见 Tab 身份建模叠加，测试也只覆盖了重新点击入口，没有覆盖 Tab 已打开时切换 session。
- [x] 数据与状态转换：可见源码管理 Tab 改为项目级稳定 key，同一会话树只保留一个；Provider 从 Run 当前 `selectedSessionKey` 投影 `workspacePath`，页面 locator 仅作为 deep-link 回退。session 点击在同一事件中提交 React 页面状态、最新页面 ref、URL locator 与 Run 选择，避免旧 main locator 覆盖已选 worktree。只有 `projectId + normalized workspacePath` 改变时才切换底层 SourceControl session；main 节点之间或同一 worktree 节点之间切换是 identity no-op。底层继续复用现有 24 项 repository/workspace LRU，返回某个位置时恢复其内部页签、历史分页/选择/滚动和 commit 草稿。
- [ ] 验收：纯状态测试固定稳定 Tab key、同 identity 引用不变和跨 worktree 投影；DOM 测试固定已打开 Tab 自动跟随路径且不产生第二个同名 Tab；SourceControl Store 测试固定 Windows 规范化路径与不同 worktree 视图状态隔离。完成 TypeScript、定向测试、生产构建以及真实浏览器 normal/narrow/re-expand 验证后勾选。
- 性能与过度设计评审：不新增 Context、持久字段、缓存、队列、请求或 Git watcher；只抽取轻量路径 identity helper，并复用已有 Store。会话节点切换只做规范化 identity 比较；工作位置相同时保持稳定 scope/context，不触发源码管理订阅切换或 Git I/O，位置变化时仅目标 SourceControl session 按原规则按需加载。

## 2026-08-26：快速对话上下文操作栏响应式收缩

- [x] 根因与方向：工作空间、工作位置和分支作为三个独立上下文选择器的设计正确，但现有信息栏只允许文本截断，没有把完整控件投影成紧凑图标态，属于正确设计的响应式实现不完整。继续保留三个一步直达入口，不增加统一“更多”菜单或按截图尺寸打补丁。
- [x] 实现：信息栏建立命名 CSS container；窄档三个触发器均保持 28px 图标态，中档只恢复工作空间标签，宽档再恢复工作位置与分支标签及箭头。复用现有 shadcn/Radix Select、DropdownMenu、Popover + Command 和项目 Tooltip；同一控件实例只切换 CSS 展示，图标态提供包含当前值的 Tooltip 与 `aria-label`，菜单打开时抑制对应 Tooltip。
- [x] 点击时序修复：紧凑分支图标的 Tooltip 不再在 pointerdown 阶段先行退出，而是与后续 Popover open 在同一 React 提交中收敛；触发器使用受控 Popover open 的独立数据属性保持主题强调态，不再读取会被 TooltipTrigger `closed` 覆盖的共享 `data-state`，消除“变深—恢复—弹出”的中间帧。键盘 click、溢出 Tooltip 和原 Popover/Command 生命周期保持不变。
- [x] 关闭焦点修复：指针点击分支项完成异步切换后，Radix Popover 原本将焦点还给紧凑分支触发器，其 focus Tooltip 因而重新打开并持续显示。现在沿用工作空间/工作位置选择器的输入方式契约：指针关闭时阻止自动还焦、清理 Tooltip 并移除触发器焦点；键盘关闭仍保留 Radix 默认还焦。不使用 timer、延迟或第二套菜单。
- [x] 验收：3 个定向 Vitest 文件共 32 项通过，固定三档 class、三个控件稳定 identity、当前值无障碍名称、图标 Tooltip、pointerdown→click 时序、指针关闭不还焦与键盘关闭还焦；TypeScript 与 Web 生产构建通过。内置浏览器按操作栏实际宽度验证约 293px 全图标、413px 仅工作空间标签、614px 全标签及重新拉宽恢复；浅色与系统深色仿真均验证 Tooltip 已显示后点击分支图标会当次打开 Popover、同步关闭 Tooltip。在 28px 紧凑分支触发器中实际切换分支后，Popover 与 Tooltip 均收起且焦点不回图标；Escape 键盘关闭则正常还焦。原分支、临时视口、颜色仿真和页签均已恢复或清理。
- 性能与过度设计评审：响应式完全由浏览器 CSS container query 计算，不增加 ResizeObserver、React 尺寸 state、effect、依赖、缓存、队列、请求或重复选择器实例；窗口连续缩放不触发 React 渲染，分支 snapshot 读取次数和既有有界 Store 不变。每个标签只增加固定 class 与无障碍属性，DOM 数量保持常量，无专项 benchmark 必要。

## 2026-08-25：Release profile WebView DevTools 诊断包

- [x] 根因与方案：现有 default/wb 渠道构建、Tauri overlay 和 updater 隔离设计正确，但只有渠道维度，没有用于复现生产 WebView 问题的诊断能力维度；普通 release 又未启用 Tauri DevTools。新增正交 `--devtools` 构建选项和 `support-devtools = ["tauri/devtools"]` Cargo feature，不新建诊断渠道，也不使用会改变优化行为的 debug profile。
- [x] 构建接口：`npm run build -- --devtools`、`npm run build:wb -- --devtools` 和 `npm run build:channel -- <channel> --devtools` 统一经现有 `build-channel.mjs` 追加 `--features support-devtools`；既有 `critical` 位置参数继续兼容，同时支持 `--critical`，未知参数直接失败，避免拼写错误静默产出普通包。
- [x] 发布边界：诊断 overlay 显式设置 `bundle.createUpdaterArtifacts=false`，且 post-build 不复制签名更新包、不生成或覆盖渠道 `latest.json`。普通本地渠道构建与 GitHub 正式发布参数不变，默认不启用 DevTools；诊断包只用于定向支持，不进入正式 updater 链路。
- [x] 回归与评审：Node 接口测试固定普通构建参数不变、DevTools feature 透传、critical 兼容、未知参数拒绝，以及诊断 overlay 在保留渠道 bundle targets 时关闭 updater artifacts；`npm run test:channel-config` 5 项与 `cargo check -p gold-band-desktop --features support-devtools --locked -j 1` 均通过，仅保留项目既有 dead-code warnings。普通构建没有新增运行时代码、I/O 或内存开销；诊断构建仅复用 Tauri 官方能力，不新增依赖、状态机、持久字段、缓存、队列或并发机制，复杂度与实际支持需求匹配。

## 2026-08-26：继续并发送 prompt 显式续接此前任务

- [x] 根因与方案：既有 `resume_with_message` 已正确区分可见用户输入、Runtime hidden 控制段和三种 `OutputEmissionMode`，但控制文案只要求执行本消息，没有显式要求随后继续此前未完成任务，属于正确设计下 prompt 实现不完整。直接完善现有中英文条件模板，不修改 Runtime 状态、checkpoint 或消息投影。
- [x] 提示契约：三个分支统一要求“先完整执行本消息中的用户指令，然后继续完成你之前的任务”；PostTurn 在任务完成后再由后续独立 turn 归一化，InlineControl 在任务完成后再按当前契约输出 artifact，无 contract 时不提 artifact。
- [x] 回归验收：模板分支单元测试 1 项、固定工作流继续接口 1 项、AI-DYNAMIC merge/child 继续接口 2 项通过；`cargo fmt --all -- --check` 通过。测试固定中英文续接语义、artifact 动作顺序，以及 visible 用户消息与 hidden Runtime 控制段的投影边界，仅保留项目既有 3 条 dead-code warning。
- 性能与过度设计评审：只修改现有常量模板并增加常量级字符串断言，不新增数据结构、状态、依赖、持久化、缓存、队列、锁、I/O、扫描或渲染订阅；渲染复杂度和 prompt 分支数不变，无需 benchmark。

## 2026-08-26：会话信息角标按宽度渐进收起

- [x] 根因与方案：会话详情 composer 的附着信息 tab 已正确集中展示运行状态、累计时间、上下文占用、工作树和分支，但外层强制单行、子项允许收缩，又没有信息优先级或溢出出口；会话栏变窄时所有文本同时被截成半截。该问题属于正确信息模型下响应式投影实现不完整，不修改 Runtime、Git、workspace 或 usage canonical state。
- [x] 实现：`AcpUsagePanel` 观察 content rail 的真实宽度，以 `560 / 440 / 340px` 三个集中常量投影四个离散档位；按“分支 → 工作树 → 上下文窗口”从右向左将完整原控件移入 shadcn `Popover`，运行状态和会话累计始终行内。ResizeObserver 每动画帧最多处理一次，同一档位不提交 React state；每项只挂载一次，避免复制分支选择器、snapshot 请求与焦点状态。分支选择器保留原 Popover，允许在外层“更多”内继续打开；外层 Popover 默认左对齐三点按钮，移入的分支触发器占满其内容宽度并保持左对齐，行内紧凑样式不变。
- [x] 打开焦点修复：外层 Popover 原本会自动聚焦内容中的第一项，工作树 Tooltip 因其可键盘聚焦而立即弹出。现仅阻止外层的自动首项聚焦，让焦点保留在“更多”按钮；用户主动 Tab 后 Tooltip 仍可访问，嵌套分支 Popover 仍保持正常层级。该缺陷属于正确可访问性设计下组合浮层的初始焦点契约不完整，不通过禁用 Tooltip focus 或延迟补丁规避。
- [ ] 回归与验收：纯函数测试固定三个边界，DOM 测试固定正常、分支收起、工作树收起、上下文收起、打开时不自动显示工作树 Tooltip、嵌套分支 Popover 和重新拉宽后的唯一挂载；完成 Web TypeScript、生产构建及内置浏览器 normal/narrow/re-expand、长分支、浅色/深色验证后勾选。
- 性能与过度设计评审：宽度变化只在本地信息栏发布四值枚举，普通 Timeline、Markdown、Composer 草稿和右侧工作区不订阅；不增加 IPC、Git I/O、轮询、缓存、持久字段、Context、领域状态机或依赖。现有信息项和 shadcn/Radix 浮层已足够表达需求，无需内容测量算法、重复 DOM 或新的 responsive framework；焦点修复只是 Popover 标准事件的一次同步 `preventDefault`，不增加渲染或事件监听。
