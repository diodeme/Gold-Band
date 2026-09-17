# 工作空间与任务记忆及 CICD 工作流实施方案

日期：2026-09-09。状态：共享记忆、正式 CICD 角色及 WB 工作流对接与本地验收完成；真实模型及 WeTest 部署端到端验收待补。

产品行为以 [工作空间与任务记忆及 WB CICD 工作流](../../产品设计文档/runtime/workspace-task-memory.md) 为唯一真源。本文件维护实施顺序、技术检查项和验证进度，不复制第二套业务规则。

## 1. 交付范围

一次性交付通用两级记忆、项目记忆设置抽屉、模型读写工具与上下文注入，以及 WB 专属默认项、CICD 角色和“开发构建部署工作流”。实现代码已落盘，最终验收进度如下；验证使用隔离临时文件和浏览器预览数据，不修改用户实际记忆文件。

正式 CICD 提示词由其他同事提供。初始角色按产品设计中的最小契约交付；配置不完整必须明确停止，不能以占位成功完成工作流。后续真实外部部署验收依赖具体调用契约和可用测试环境。

## 2. 实施前检查

- [x] 完整读取适用的 UI、数据加载、前端性能、状态生命周期规则及相关技术 skill。
- [x] 回溯现有 project_id、task locator、数据根解析与初始化生命周期，定位正确文件归属。
- [x] 查明现有 JSON、原子写入及并发校验能力，优先复用成熟依赖和本地 helper。
- [x] 查明模型可调用工具的注册与作用域绑定机制，验证 Direct、Workflow、AUTO 的提交绑定与 ACP stdio capability 接入；真实 MCP 子进程完成握手与工具调用。真实模型端到端行为另列待验项。
- [x] 查明各模式上下文准备、继续执行和重试入口，保证读取失败时不继续提交缺少记忆的执行。
- [x] 核实默认轻量工作流模板、可选入口、WB 可见性及节点失败后的人工重试能力。
- [x] 明确文件 schema、字符计数、序列化字节计数和冲突校验契约。避免用文件级冲突拒绝不同 key 的合法合并。
- [x] 明确普通模式已初始化后切换 WB 的默认项初始化时机：已有文件不补项，仅 WB 首次创建文件时写默认项。

以上技术检查不能改变已确认产品行为；确需新增产品决策时明确提出，不自行补入业务约定。

## 3. 数据与接口

- [x] 建立用途明确的条目、作用域 locator、读取结果和写命令 DTO；文件及接口不可依赖显示名称查找。
- [x] 统一维护字段和容量限制，不在 UI、工具和运行时分别硬编码。
- [x] 提供作用域读取、有效记忆合并、按 key 保存与删除能力，返回最新权威结果。
- [x] 复用既有机制实现短临界区中的冲突校验、合并和原子落盘；明确崩溃与失败时原文件的保留行为。
- [x] 幂等初始化两级文件与 WB 默认条目，正常读取不承担反复修复或补项工作。
- [x] 返回结构化的字段错误、容量错误、冲突、文件损坏和 I/O 错误，前端负责对客文案。
- [x] 设置页与模型工具统一调用领域服务；模型不直接覆盖文件。

接口测试覆盖：

- [x] 同名 task 跨 project 不串数据，错误 locator 不越界。
- [x] 任务优先、任务不存在时继承、任务空值不回退。
- [x] 跨重试和重新运行保留，新 task 不继承其他任务参数。
- [x] WB 首次初始化创建默认项、初始化重复调用幂等、删除后不补回。
- [x] 不同 key 同时更新均保留；同 key 迟到修改被拒绝并返回最新值。
- [x] 写入失败不返回保存成功，文件损坏不清空，读取失败不伪装为空。
- [x] Unicode 字符与序列化字节边界、条目数与总容量边界、项目更新后任务组合超限。
- [x] task 删除后迟到 read/write 返回 locator 错误，且不重建 task 目录或记忆文件。

## 4. Runtime 与提示词

- [x] 各执行模式按完整 locator 获取记忆，启动与重试前合并并注入来源和路径。
- [x] 提供运行期间刷新与写入工具，工具成功结果建立在持久化成功之后。
- [x] 模板与固定规则放入 `src/prompts/zh-CN/`、`src/prompts/en/` 对称目录；system/user 职责不变。
- [x] 将记忆标记为数据，写入权限和参数复用规则由统一模板管理。
- [x] 读取或容量错误阻止当前执行推进；映射到现有可恢复错误与重试机制，不增加独立记忆生命周期。
- [x] 测试前序写入到后序读取的接口链，覆盖重试刷新、任务纠正不修改项目默认值和显式 workspace 写入；不等同于真实模型行为验收。
- [ ] 通过可重复流程验证同 key 有效值复用，明确任意模型行为无法由提示词绝对保证。

## 5. 项目记忆 UI

- [x] 工作空间书本图标与 Tooltip，并入新增/删除悬浮操作组，按需打开“项目记忆设置”Sheet。
- [x] 使用已有 shadcn/ui copy-in 基础控件实现三字段编辑、逐条保存取消与删除确认。
- [x] 未保存草稿关闭提醒、目标级加载和错误、冲突后的最新值展示。
- [x] 保持当前工作空间作用域，切换后拒绝迟到读取或保存投影覆盖。
- [x] 冲突采用最新值时按权威 key 重投影；远端重命名后旧行移除，不产生重复 key。
- [x] 不新增任务记忆管理界面，不将记忆加载加入工作空间列表首屏。
- [x] 执行相关接口与 DOM 测试、前端类型检查和生产构建。
- [x] 启动前端，使用 iab 验证正常与窄宽度、长文本、明暗主题及关闭草稿流程；冲突/失败使用 DOM 测试，结束清理测试资源。

## 6. WB CICD 与工作流

- [x] 注册 WB 专属 CICD 内置角色，接入双语正式 WeTest 提示词。
- [x] 从默认轻量工作流机制构建新内置模板，保留前序行为，验收成功后进入 CICD。
- [x] CICD 节点默认启用人工 check，移除默认 output / success_condition / cicd-result；不自动重试部署，失败通过既有当前节点人工恢复路径处理。
- [x] 移除无条件未配置失败，保留单次提交与人工恢复；自定义节点显式配置 AI 输出验证时保留既有产物与控制结果校验。
- [x] 测试普通模式不可见 WB 角色和模板，WB 可见；通用记忆在两种产品模式下均可用。
- [x] 测试可选拷问、验收失败修正、验收成功直接进入 CICD、CICD 成功保留与失败恢复。
- [ ] 具备真实 Agent、WeTest 环境和部署授权后，验收多子系统、部分成功与人工恢复；本地契约测试不替代真实平台验收。

## 7. 性能与过度设计验收

- [x] 确认没有新增向量库、语义匹配、记忆数据库、无界缓存、后台同步队列或平行身份。
- [x] 节点只读取所属项目和任务两个文件，IndexMap 合并 O(P + T)，无历史扫描和逐条 N+1 请求。
- [x] 只在打开设置时加载记忆，保存局部收敛，不触发全局列表刷新。
- [x] I/O 不阻塞 UI 或事件线程；锁不覆盖模型调用、外部网络和用户等待。
- [x] 在容量上限验证上下文组装、读取量和输入开销；32 KiB 数据边界通过，ASCII 完整上下文小于 36 KiB，真实 token 数取决于 provider。
- [x] 记录测试结果、剩余契约依赖及真实部署验证范围，同步更新产品设计文档。

## 8. 当前记录

2026-09-09 main 合并：已合入 `origin/main` 的 `ce288976`，产生合并提交 `78ea8d0e`。保留当前分支历史及未提交记忆实现；侧栏布局冲突采用远程后续修复，保留只读 Demo 标记；正式 CICD 双语文本取远程版本，移除重复占位注册，并将 catalog 测试收紧为 WB 恰好一个角色。验证：`memory_wb_catalog` 1 项、侧栏/只读 Demo 12 项、前端 TypeScript 检查及 iab `/chat` 冒烟检查通过，临时浏览器页已关闭，无未解决 Git 冲突。正式角色已到位，但它的记忆结构与当前通用记忆契约不同，且初始停止保护仍在；本次仅合并，不将部署接入标为完成。未提交修改及原占位文本保留在合并前 stash 备份中。

2026-09-09 追加调整（完成）：按已确认方案拆分稳定记忆规则与每轮参数投影。根因为原模板混合不同生命周期的内容；复用现有 system prompt 与隐藏块通道，删除恢复会话记忆特判，不增加状态、依赖或缓存。修改前 `memory_invocation` 在“per-submission memory data must not repeat stable write rules”断言稳定失败（0 passed / 1 failed），与根因一致；修改后相同测试及领域回归共 11 项通过，覆盖双语、各模式、新建/继续会话、更新后 system prompt 不变和数据仅注入一次。`cargo check -p gold-band --tests -j 1` 通过，包含新增 ACP capability 组合单元测试的类型检查，未宣称执行全部 lib 单元测试。

最终容量复核：因现有桌面开发进程占用 Cargo 产物锁，停止本次等待的测试命令，使用 rustc 与已编译依赖直接编译当前 `tests/memory_domain.rs`，其引用的生产记忆源码及 10 项测试全部通过；包含规则模板与数据模板合计小于 36 KiB 的 ASCII 上限样本。临时 exe/pdb 已清理，保留用户桌面进程。本次不修改前端，未重复浏览器验证。文件读取仍为两个有界文件，合并 O(P + T)，无额外 I/O；规则不随每轮参数投影重复传输，无需额外 benchmark。现有生命周期分层规则已覆盖该原则，不新增重复规则。

实现路径：`src/memory/` 为唯一文件领域服务；`src-tauri/src/memory.rs` 为设置 IPC；provider 提交前统一刷新，ACP 恢复会话补最新隐藏上下文；`ProjectMemorySheet` 逐行管理草稿，`WorkspaceShell` 持有抽屉目标，避免响应式卸载丢稿。MCP 使用 rmcp，跨进程锁使用 fs2，落盘复用现有 atomic-write-file。

已执行：首轮 `cargo test -p gold-band --lib memory` 13 项通过，包含 CICD 停止保护、模板路由和 ACP 恢复上下文。最终独立集成测试 `memory_domain` 10 项、`memory_mcp` 1 项、`memory_wb_catalog` 1 项、`memory_invocation` 1 项全部通过，覆盖精确容量、写入故障注入、真实 MCP 子进程、全模式绑定和重试刷新。前端四个相关测试文件共 22 项通过；TypeScript、Vite 生产构建及 `cargo check -p gold-band-desktop -j 1` 通过（仅既有编译/构建警告）。

测试资源：iab 临时页与 1428 端口 Vite 进程已关闭，viewport override 已重置。Rust 单体 lib 单元测试最终重编译遇到机器内存不足，因此通过独立集成入口编译同一份记忆领域源码及私有故障注入测试；没有宣称全仓测试通过。未修改仓库构建配置或系统设置。测试工作空间使用临时目录隔离。

过度设计与性能复核：复用现有身份、持久生命周期、原子写入和 copy-in 控件；新增依赖仅承担标准 MCP 协议及跨进程文件锁。每次提交读两个有上限文件，线性合并，设置按需加载，无新增缓存或队列。待验范围为真实模型是否按提示复用参数，以及正式外部部署契约，不影响当前未配置时明确停止的最小行为交付。

最终测试代码整理后，`cargo check -p gold-band --tests -j 1` 通过，包含 lib 单元测试及全部集成测试目标的类型检查；此结果不等同于执行全仓测试。

历史：2026-09-09 文档阶段保存访谈共识、交付边界和实施清单；本记录已更新为实现后的实际验收结果。

## 2026-09-09 正式契约对接

已替换此前占位保护与独立配置格式，当前契约以双语 CICD 角色及共享记忆设计为准。历史合并、最小角色与停止保护记录仅描述当时状态。

- [x] 双语角色改用 memory_read / memory_write，任务覆盖、空值不回退，默认写任务，项目复用需明确要求。
- [x] 按真实子系统 ID 隔离字段，task 选择不等于授权；列表采用单字段 JSON 字符串，不增加存储 schema。
- [x] 删除无条件未配置失败，保留单次调用、产物校验、人工恢复、已有外部 ID 核对及禁止自动重放。
- [x] 先红后绿验证：旧角色缺少共享读取工具、成功被占位失败覆盖，两项失败与根因一致。
- [x] 完成本次 Rust、前端检查并记录结果；真实平台操作不在本地测试范围内。

自评审：复用现有机制，不新增抽象状态或依赖；两个有界文件、O(P + T) 合并、逐 key 写入，沿用 100 条及 32 KiB 上限，无历史扫描、无新增缓存或队列。字段容量不足明确阻塞，不绕过限制。
输出契约补充：CICD 的单次提交与普通节点的执行后产物补问不兼容。最小测试从 WB 模板取得实际 Worker 后，原契约返回 PostTurnProjection 而非 InlineControl，稳定失败。现统一按内置角色身份选择单次行为：静态与动态节点在同次执行内声明并提取既有产物协议；schema、成功条件与运行状态归属不变，普通角色继续原输出流程。仅将原有输出契约构造函数拆入小模块供独立接口测试，无新增模型、状态或 I/O。

2026-09-09 对接验收完成：Rust 共 19 项独立测试通过（memory_domain 11、memory_mcp 1、memory_invocation 1、memory_wb_catalog 1、cicd_profile_contract 1、cicd_recovery 3、cicd_output_contract 1）；三处失败证据均已由红转绿。`cargo check -p gold-band --tests -j 1` 通过，包含动态输出契约断言的类型检查，不宣称运行全部 lib 单元测试。前端 3 个测试文件共 19 项通过，TypeScript 检查、Vite 生产构建、Rust 修改文件格式检查与 diff 空白检查通过；构建仅有既有未使用函数、分包及大 chunk 警告。iab 在本次 1428 服务的 `/chat` 页面完成冒烟验证，主验证页面和 Vite 进程已关闭；先前 1420 连接失败的临时错误页因浏览器 data URL 策略无法重新获取关闭，未绕过策略。未调用真实 WeTest 构建、部署或审批，也未验证 EXE 与真实模型的端到端执行。现有规则已覆盖数据归属与生命周期原则，不新增规则。

## 2026-09-15 生命周期与冲突投影修复

根因：任务记忆初始化把“文件不存在”统一解释为首次创建，且落盘 helper 自动创建父目录；长期存活的 MCP 子进程可在 task 删除后复活任务目录。项目记忆设置采用冲突最新值时仅取消当前行，没有按权威 key 重建列表，因此远端重命名会留下旧 key。

- [x] 先建立两项最小失败证据：删除 task 后 `memory_domain` 的 read 返回成功并重建快照；冲突 latest 将 `plan` 改为 `renamed` 后，前端仍保留 `plan` 行。
- [x] 锁内重新校验 task locator，并使用不创建父目录的原子记忆落盘；任务删除持有同一项目级 memory 锁。
- [x] 保存、重命名和采用冲突最新值统一按权威 record 重投影行，替换旧 key 并去重。
- [x] 修复后同一 `memory_deleted_task_is_never_recreated` 与冲突重命名 DOM 测试转绿。

自评审：复用现有 project-level file lock、atomic-write-file 和前端行状态，不新增 tombstone、缓存、队列或平行业务身份。锁仅覆盖短暂文件操作和 task 目录删除；记忆仍只读取两个有界文件，复杂度不变。

验证：后端记忆、MCP、调用绑定与 CICD 目标测试合计 20 项通过；前端项目记忆与侧栏生命周期测试 14 项通过；TypeScript 检查、Vite 生产构建、`cargo fmt --all -- --check`、`git diff --check` 和 `cargo check -j 1 -p gold-band-desktop` 通过。桌面编译仅有既有 dead-code 警告。

## 2026-09-15 入口交互与 CICD 判定调整

- [x] 项目记忆入口改用书本图标，并与新增会话、删除工作空间入口共用悬浮/聚焦操作组。
- [x] WB 内置模板的 CICD 节点改为人工 check，移除默认 output、success_condition 和 cicd-result。
- [x] 保留 CICD 单次提交、人工恢复、禁止自动重放，以及自定义节点显式配置 AI 输出验证时的 InlineControl 能力。
- [x] 更新 DOM 与模板接口测试，确认旧任务 authoring workflow / Run snapshot 不做隐式迁移。

验证：`workspace-sidebar-width-hydration` 5/5、`memory_wb_catalog` 1/1、`cicd_output_contract` 1/1、WB 模板单测 1/1、`cicd_profile_contract` 1/1、`cicd_recovery` 3/3 通过；`npm run web:build`、`cargo fmt --all -- --check` 与 `git diff --check` 通过。iab 实际确认书本图标、三按钮同组、聚焦展示、Tooltip 入口及项目记忆 Sheet 打开/关闭，并检查深色模式下仍可见，结束后已恢复跟随系统主题、关闭页面与测试服务。

自评审：复用现有 Button、Tooltip、悬浮操作组与 manual check 生命周期，不新增状态、依赖、接口、缓存或队列。改动为常数级 DOM 与模板字段调整，不进入数据加载或渲染热路径，无需 benchmark。存量任务不迁移，避免改写 canonical authoring 和 immutable snapshot。
## 2026-09-16 内置记忆 MCP 与目标 key 冲突收敛

根因分为两类：重命名到已存在 key 时，后端 CAS 正确拒绝写入，但前端把“目标权威记录”误投影成“重命名已成功”，属于正确设计下的消费端逻辑错误；MCP 则把持久配置、临时健康状态和正式会话进程混为同一生命周期，造成启动批量探活、重复写 settings 以及无法为内置记忆区分基础定义与 task 绑定，属于根本边界设计缺陷。

- [x] 先用最小 DOM 失败测试复现 `plan → 已存在 renamed` 后“采用最新值”吞掉 `plan`；后端 `memory.conflict.params.reason` 区分 `revision_mismatch / target_exists`，前端按原因保留源行并更新目标行，同一测试转绿。
- [x] MCP 拆成 definition / diagnostic / executable session snapshot 三层；删除 `McpManager` 内第二状态缓存和启动批量探活，列表与会话配置读取不产生外部 I/O。
- [x] 内置 definition 使用标准 managed stdio 卡片，启动幂等 reconcile 并保留 enabled；配置未变化返回 `Unchanged` 且不写 settings。卡片可开关、诊断、查看工具，不可编辑或删除。
- [x] 共享记忆基础 args 只有 `--gold-band-memory-mcp`；无绑定进程完成 initialize/initialized/tools-list，业务工具返回 `memory.context-required`。Direct、Workflow、AI-DYNAMIC 和人工续聊在 prompt 准备时注入当前 task 绑定，且只存在于本次 invocation。
- [x] enabled 同时控制 MCP 与双语记忆 prompt；关闭时二者都不传。正式 stdio 由 ACP Agent 在 session/new/load/resume 阶段启动，Gold Band 不常驻第二份连接。
- [x] 上下文管理卡片使用“尚未检测 / 最近检测通过 / 最近检测失败”等诊断文案，不把短进程检查显示为“正在运行”；页面刷新不自动批量探活。浏览器与产品 Demo 均加入标准内置记忆卡片 fixture。
- [x] 完成受影响面回归、前端生产构建、生成物刷新与浏览器 deep link 交互验证。

验证记录：

- Rust：`memory_domain` 12、`memory_mcp` 2、`memory_invocation` 2、`memory_wb_catalog` 1、`provider_prompt_bundle` 31、`ai_dynamic_node` 38、`worker_bootstrap` 21、lib `mcp::` 10 项全部通过；`cargo fmt --all -- --check`、`cargo check --workspace`、`cargo metadata --locked` 通过（Cargo.lock 与清单一致，本次未新增依赖）。无绑定子进程完成 initialize/initialized/tools-list 后返回 `memory.context-required`；带绑定子进程仍验证跨进程读写与 stale revision 拒绝。
- 前端：`project-memory.test.tsx`、`mcp-server-card.test.tsx`、`demo-api.test.ts` 共 21 项通过；`npm run web:build`（TypeScript + Vite）通过；Agent catalog 契约 7 项通过；`resources/acp-registry.snapshot.json` 与 `resources/agent-catalog.json` 按上游 Registry 1.0.0 重新生成（claude-acp 0.78.0、codex-acp 1.12.0）。
- 浏览器：iab 与本机已连接 Chrome 在本环境均不可用，按规则回退到批准的 `agent-browser`，并从本工作树启动 1433 端口 dev server。deep link 进入“上下文 → MCP 管理 → 内置 MCP”验证：内置卡片显示“Gold Band 共享记忆 / Stdio / gold-band-desktop”；仅提供开关、诊断、工具三个入口，无编辑与删除；手动诊断后状态提示为“最近一次 MCP 配置检测通过”；进入页面仅刷新不会自动探测；关闭开关后诊断计数归零，重新开启只触发一次检查。会话、浏览器、dev server、截图与临时工作树均已清理。
- 项目记忆“重命名目标已存在”冲突由 `project-memory.test.tsx` 的 DOM 回归覆盖（保留源 key、更新目标 key、不吞行）；浏览器预览的内存适配器不模拟 CAS 冲突，未用页面伪造该路径。
- 已知既有红灯（非本次合并引入）：在 `origin/main` 干净工作树复现同样 5 条前端契约失败（native-title-tooltip-contract、context-management-loading、scheduled-task-composer、acp-activity-batch、acp-runtime-continue-submit）；`chat-container-scroll-input` 仅在满载并行下偶发、单独运行通过；本 PR 未修改这些文件。

合并期机械修正：`tests/worker_bootstrap.rs` 仍断言旧版 runtime control resume 文案，而 main 已把该文案迁移到 `src/prompts/zh-CN/runtime/runtime_control_resume.md`；改为引用 `RUNTIME_CONTROL_RESUME_ZH_CN` 权威常量，避免同类陈旧字符串再次漂移。

## 2026-09-16 合并回 PR 分支（12 项冲突）

远端 PR 分支在评审期间自行前进（`15c33867` 合并最新 main，并新增 `87334550`、`f0dd11f5`），与本分支形成 12 项冲突。按“远端更新的产品决策 + 本分支的记忆/MCP 迁移”逐项收敛：

- 提示词与 `profiles.rs`：保留 task 级 `cicd.build.*` + 逐子系统 `cicd.deploy.<S>.*`、每次 run 重新确认、禁止直接读写记忆文件；同时吸收远端更新的“节点最终结果由 runtime 结果模式决定”措辞。
- `src/app/mod.rs`、`tests/cicd_output_contract.rs`、`tests/memory_wb_catalog.rs`：采用远端 `87334550` 的决定，WB 默认 CICD 节点为人工 check，移除默认 output / success_condition / `cicd-result`；自定义节点显式声明输出时的 InlineControl 仍由独立单测覆盖。
- `ConversationSidebar.tsx`：采用远端的书本图标与悬浮操作组，并恢复本分支的 `stopPropagation`，保证点击只打开记忆抽屉。
- 两个资源快照由 `npm run agent-catalog:refresh` 重新生成；文档保留两边记录并同步 key 迁移说明。

验证：`cargo check --workspace --all-targets`、`cargo fmt --all -- --check`、`git diff --check` 通过；默认渠道 `memory_domain` 12、`memory_invocation` 2、`memory_mcp` 2、`memory_wb_catalog` 1、`cicd_profile_contract` 1、`cicd_output_contract` 1、`cicd_recovery` 3、`provider_prompt_bundle` 31，`GOLD_BAND_RELEASE_CHANNEL=wb` 下同批 18 项，lib `mcp::` 10、`ai_dynamic_node` 38、`worker_bootstrap` 21 全部通过；前端 `project-memory`/`mcp-server-card`/`demo-api`/侧栏 27 项与 `npm run web:build` 通过。`context-management-loading` 仍是 main 既有红灯（App.tsx `listen<...>` 字符串断言），非本次引入。

过度设计复核：复用现有 `McpServerConfig`、managed 卡片、ACP `mcpServers` 与 `WorkerInvocation`，不新增通用占位符模板、常驻代理、第二种卡片类型、持久诊断字段、缓存、队列或新状态机。性能复核：启动外部 MCP 握手从 O(N) 降为 0；definition reconcile 为有界列表线性比较，未变化零写盘；显式诊断只启动单个短进程并统一回收进程树；会话仍由 Agent 承担原本就需要的单次 stdio 启动成本，无额外 N+1 或历史扫描。

## 2026-09-17 WB 需求身份与固定记忆作用域

根因：需求 ID / 名称是开发测试生成三行提交所需的规范输入，但任务记忆没有固定 key 和初始化链路；同时 CICD 参数只有“默认写任务”的通用规则，无法保证每个参数写到正确作用域。

- [x] WB 采访和拷问在角色流程前使用 `memory_read` 只检查任务作用域 `storyId/storyName`；缺失、为空或不成对时，以“不存在 / 其他（用户自行输入）”语义提问。
- [x] 选择不存在时写 `storyId=0` 和从需求内容提取的最多 40 字符简短名称；选择其他时要求同时提供 ID 和名称。两个 key 均写任务作用域并写后核验。
- [x] 开发测试在采访或拷问关闭导致身份缺失时自动兜底补齐，不向用户询问提交消息。
- [x] 固定作用域：`subSysId*` 写工作空间；`storyId/storyName` 与全部 `cicd.*` 生效参数写任务。
- [x] 记忆领域接口测试新增跨节点任务身份读取，确认任务值优先并在重新构造服务后保持。
- [x] 记忆工具不可用、写入失败或写后核验失败改为可降级流程：向用户询问或确认参数并继续，明确数据未持久化，不直接修改记忆文件。

验证：WB 渠道 `memory_domain` 13/13、`wb_workflow_profile_contract` 1/1、`cicd_profile_contract` 1/1 通过；default 渠道确认不注入 WB 补充规则；`cargo check -p gold-band --tests -j 1` 通过。

自评审：复用现有 `gold-band-memory` MCP、逐 key CAS、Profile 渠道常量和原子写入，不新增记忆文件、状态机、缓存或队列。每次身份检查仍只读取当前项目与任务两个有界文件，复杂度 O(P + T)。
