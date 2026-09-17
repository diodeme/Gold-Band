# CI/CD 内置角色实施方案

## 方案

新增仅 `wb` 内部渠道提供的静态双语 `pf-builtin-cicd`，复用现有角色机制供用户绑定自定义工作流节点。参考用户提供的 wetest-cli 资料整理命令，不在本次执行资料中的安装、构建或部署指令。无需额外开源依赖；现有 CLI 与 Profile 机制覆盖需求。

正文默认执行构建 + 部署，通过交互确认按构建或包名部署，推荐按构建。回归部署 / 审批、触发和查询自测以及跑批仅供用户选配，不主动执行。工作空间 `subSysId1` 等提供成员清单；task 的唯一构建使用 `cicd.build.*`，部署按真实子系统 ID 的 UTF-8 百分号编码 S 使用 `cicd.deploy.<S>.*`。所有参数通过共享记忆工具逐 key 读写，已有参数只作预填，每次 run 都重新确认实际范围。保留参数来源、真实终态、有界轮询、响应丢失核实和恢复去重原则。元数据及正文中英文同步维护；CICD 加入 WB 专属工作流的验收成功后节点，普通模式不可见，不新增 runtime 状态。

构建、物料 / 镜像推送或部署前的业务仓库提交门禁在双语角色正文中维护：相关范围来自原始需求、当前 task / goal、显式前序产物和用户指认；发现相关未提交改动时，向用户确认文件范围和三行提交消息后，仅按具体路径协助 git add / git commit 并复查，不纳入无关改动。相关提交已存在但未推送时，核对分支、远端与待推送提交，用户确认后执行普通 git push 并核实远端分支包含相关提交。三行提交中的 Conventional Commit 使用标准类型 token 与中文描述；需求 ID / 名称按原始需求、用户确认、默认值顺序解析。代码 push 与物料 / 镜像推送是不同操作。

## 实施进度

- [x] 阅读现有角色、runtime 输出契约和 wetest-cli 命令参考。
- [x] 编写中英文角色正文，修正参考资料配置文件名不一致的问题。
- [x] 注册稳定 ID、双语摘要和静态正文映射。
- [x] 同步产品设计文档。
- [x] 完成 Profile 接口测试与提示词检查，记录结果。

## 验收与性能

验收覆盖中英文 list/show 返回正确正文、稳定 ID、内置只读保护、默认工作流不自动引用 CI/CD。按接口测试结果记录完成状态；不调用真实 WeTest 写操作，不以测试通过代表平台执行通过。

过度设计审查：只维护角色资源、seed 渠道元数据和共享记忆中的既有字符串条目，复用现有构建渠道、状态、接口和命令能力，不增 runtime 数据模型、第二套子系统身份或整份配置 JSON。一个 task 级 `cicd.build.*` 表达一次构建和多应用推送，`cicd.deploy.<S>.*` 表达各子系统独立模板与目标。性能审查：内置目录最多 10 项，正文构造前过滤；共享工具只读取当前 workspace/task 两个有界文件，配置与命令数量为 O(所选子系统数)，无额外历史扫描、数据库访问、N+1、缓存、队列或无界并发。

## 初版验收记录（2026-09-09）

- `cargo test -p gold-band --lib app::profiles::tests:: -- --test-threads=1`：26 项通过，包含 CI/CD list/show 双语映射、只读保护和默认流程不引用测试。
- 首次测试因 Windows `TEMP` / `TMP` 使用 `STEVEN~1` 短路径，现有临时路径识别未隔离用户目录，两项旧测试计数失败。清理本次创建的 17 个测试文件后，仅在测试子进程中将这两个变量设为对应完整路径重跑，26 项全部通过；未修改用户的持久环境变量或路径实现。
- `rustfmt --edition 2024 --check src/app/profiles.rs src/prompts.rs` 与 diff 空白检查通过。
- 双语正文核对通过：配置文件均为项目根目录与 task 目录的 `memory.json`，四域命令、有效目标、授权继承、终态和恢复原则一致。
- 此次未修改前端交互或启动桌面客户端，未验证运行中 EXE 的角色选择，也未调用实际 WeTest 构建、部署或审批。内置资源需要重新构建客户端才能在客户端中生效。
- 最终自评审：无新增抽象、状态、依赖或后台任务；单次列表最多增加约 13.2 KB 原始正文，性能影响固定且有界。

## 职责调整（2026-09-09）

原稿把项目 / task 配置当作通用覆盖层，且主流程与选配能力并列，未准确表达业务职责。按用户澄清修正角色设计，不增加补丁式 runtime 分支。

- [x] 先增加两项接口契约测试，原正文稳定失败：缺少默认“构建 + 部署”契约，以及缺少 task `memory.json` 模板；已有 CI/CD 身份测试通过。测试夹具的编译问题修正后取得上述红灯证据，编译失败不作为复现依据。
- [x] 双语默认构建部署、两种方式交互确认、按构建推荐，以及附加操作明确选择后执行。
- [x] 项目成员清单与 task 选择分离，多子系统参数按真实 ID 隔离；通过 memory_write 逐 key CAS，全部成功后刷新核实完整参数组，禁止角色直接写文件。
- [x] 同步角色摘要及产品设计文档。
- [x] 使用同一测试确认转绿，并运行完整 Profile 测试组与格式检查。

验证仅覆盖角色文本、模板和 Profile 接口，不把静态契约测试当作真实 Agent 交互或文件生成测试；不创建当前项目 / task 的业务 `memory.json`，因为本次目标是调整内置角色。

调整后验收：在使用完整 Windows 临时路径的测试子进程中运行 `cargo test -p gold-band --lib app::profiles::tests:: -- --test-threads=1`，28 项全部通过，包括两项原先失败的新增契约测试。Rust 格式与 diff 空白检查通过。双语模板通过 serde_json 解析且完全一致，角色 ID 和注册机制保持稳定。

最终自评审：新增模板只承载用户已明确需要的多子系统构建部署参数，没有增加授权状态机、后台写入服务或缓存；正文原始大小中文约 15.8 KB、英文约 18.4 KB，增量固定有界，无需 benchmark。此次调整不修改团队共享规则，现有数据归属与权威事实源原则已覆盖配置职责分离要求。

## 动态发现补充（2026-09-09）

- [x] 双语正文明确：命令或参数有疑惑时用 `wetest <cmd> --help`，按需进一步查子命令 help。
- [x] 动态发现继续按实际副作用遵循查询自由、触发类确认；不扩大选配范围，不绕过部署 / 审批核对，不试运行触发命令探测参数。
- [x] 扩展双语 Profile 接口契约断言，同步产品设计文档。
- [x] 运行 Profile 回归与格式检查并记录结果。

复用现有发现与授权设计，只完善角色指令，没有新增状态、依赖、扫描或后台任务。提示词增加固定短文本，help 仅在有疑惑时按需调用，性能影响可忽略。

验收：`cargo test -p gold-band --lib app::profiles::tests:: -- --test-threads=1` 28 项通过，包含新增的双语动态发现与安全门契约断言；测试子进程继续使用完整 Windows 临时路径。Rust 格式和 diff 空白检查通过，未执行真实 WeTest 操作。

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

## 2026-09-11 提交门禁正文优化

- [x] 双语正文统一提交门禁小节位置，明确相关代码范围、三行格式、标准类型 token 与中文描述、需求 ID / 名称解析顺序。
- [x] 明确验收节点不执行代码 push，角色内代码 push 与物料 / 镜像推送是不同操作。
- [x] Profile 接口测试固定双语文本中的提交门禁关键契约。

自评审：仅优化静态提示词与契约测试，不新增 runtime 状态、依赖、I/O 或扫描；测试仍通过 Profile 接口读取内置正文，性能影响为常数级字符串断言。验证：`rustfmt --check tests/cicd_profile_contract.rs` 与 `cargo test -p gold-band --test cicd_profile_contract -- --nocapture` 通过，测试 1 项。未执行真实 WeTest 操作或模型端到端验收。

## 2026-09-11 辅助提交补充

- [x] 双语角色改为发现相关未提交改动时协助用户提交：仅检查相关路径状态与差异，确认三行提交消息后按具体路径执行 git add / git commit，并复查结果。
- [x] 保留交互确认、禁止 git add -A、禁止纳入无关改动和禁止修改业务代码内容的边界。
- [x] 更新 Profile 契约测试与产品、开发文档。

自评审：仍是静态角色契约调整，不新增 runtime 状态、依赖或代码执行器；Git 检查限定为相关路径的状态与差异，避免全仓库扫描或无关改动入库。验证：`rustfmt --check tests/cicd_profile_contract.rs` 与 `cargo test -p gold-band --test cicd_profile_contract -- --nocapture` 通过，测试 1 项。未执行真实 Git 提交、WeTest 操作或模型端到端验收。

## 2026-09-11 代码推送补充

- [x] 双语角色明确提交并复查成功后协助用户 push；已有相关提交但未推送时同样进入推送流程。
- [x] push 前核对当前分支、远端配置、待推送提交和实际将更新的远端分支，用户确认后仅执行普通 git push。
- [x] 禁止 force push、切换无关分支和推送未确认内容；push 后核实远端分支包含本次相关提交，失败或缺少相关提交时阻止构建部署。
- [x] 更新 Profile 契约测试与产品、开发文档。
- [x] 双语正文为 `wetest --version` 起始的环境与参数检查增加独立小节，避免延续“代码提交前置条件”的编号。

自评审：推送仍是 CICD 角色契约而非 runtime 硬门禁，不新增状态、依赖或代码执行器；检查限定为当前分支、远端配置和相关提交，push 前必须由用户确认实际远端分支与提交范围。首轮契约测试因英文断言大小写与正文不一致失败，修正断言后重跑通过。验证：`rustfmt --check tests/cicd_profile_contract.rs`、`git diff --check -- <本次相关文件>` 与 `cargo test -p gold-band --test cicd_profile_contract -- --nocapture` 通过，测试 1 项。未执行真实 Git 提交 / push、WeTest 操作或模型端到端验收。
## wb 渠道限制（2026-09-09）

根因：原先角色无条件加入公共目录，没有表达 WeTest 仅内部渠道可用的业务边界；列表与按 ID 查找均无条件提供角色。修复位于共享目录，不以 UI 隐藏掩盖执行路径仍可解析的问题。

- [x] 最小失败测试：`GOLD_BAND_RELEASE_CHANNEL=default` 下 `cicd_profile_availability_matches_build_channel` 失败，实际列出角色，预期不可见。
- [x] seed 新增 `release_channel`，CI/CD 限定 `wb`，其他角色保持公共；列表、详情及默认角色映射复用一个过滤器。
- [x] 复用构建脚本传入的编译期渠道，默认及未知渠道不提供 CI/CD，不增加可变全局开关。
- [x] 双语提示词标明内部渠道范围，产品设计文档同步。
- [x] 实际设置 `GOLD_BAND_RELEASE_CHANNEL=default` 和 `wb` 分别编译并执行 Profile 测试，各 30 项通过；包括列表、find/show、默认 ID 映射及工作流角色解析。目录测试同时覆盖 `enterprise` 和空渠道。
- [x] 使用内置 iab 访问 `http://127.0.0.1:1429/contexts`：默认 mock 显示 9 个公共角色，CI/CD 搜索结果为空。该检查验证预览页面，不冒充真实 wb 桌面接口验收；验证页及 Vite 进程已关闭。
- [x] 完成最终格式与 diff 检查：`rustfmt --edition 2024 --check src/app/profiles.rs src/prompts.rs`、`git diff --check` 均通过。

最终自评审：一个可选渠道元数据和一个集中可用目录即可表达边界，没有新增生命周期、缓存、队列、依赖或授权系统。每次最多过滤 10 条静态记录，外部渠道反而少构造一份正文，不需要 benchmark。测试采用完整 Windows 临时路径，未更改持久环境变量，未执行实际 WeTest 操作。

## 多子系统构建部署模型修正（2026-09-11）

根因：原 task 模板把 `build` 放在 `targets[]` 的每个子系统项内，隐含“一次构建只属于一个子系统”；但真实 Jenkins 构建可能一次产出并推送多个子系统包，而各子系统部署又可能使用不同模板和目标。该结构混淆了 task 级构建生命周期与子系统级部署生命周期，继续扩展会造成重复构建配置和错误复用部署模板。

- [x] 先修改最小提示词契约测试；旧模板因仍包含 `targets` 而失败，错误为 `the obsolete per-subsystem build model must be removed`，证明失败点与根因一致。
- [x] 双语模板替换为唯一顶层 `build` 与 `deployments[]`；`build.app_list` 表达一次构建并推送的多个应用，部署项分别保存 `sub_sys`、模板 ID / 名称、方式和目标。
- [x] 构建范围整体确认一次；部署逐项确认，或完整展示全部生效部署项后批量确认。每个子系统分别查询模板、发起部署并核验终态，局部失败不被其他成功项掩盖。
- [x] 使用同一契约测试确认转绿；实际设置 `GOLD_BAND_RELEASE_CHANNEL=default` 和 `wb` 分别执行完整 Profile 测试，各 30 项通过，包含渠道目录、工作流解析与双语模板结构契约。

过度设计审查：修正现有 JSON 字段归属即可表达真实不变量，不新增 aggregate、状态机、兼容层、依赖或持久化服务；开发阶段直接删除旧 `targets` 路径。性能审查：一次构建保持常数次构建与推送操作，模板查询和部署随用户选择的子系统数线性增长，这是业务要求的必要工作量；无额外全量扫描、N+1 详情加载、缓存、队列、锁或 UI 重渲染。

验收使用完整 Windows 临时路径运行，未修改持久环境变量。`default` / `wb` 两组测试均为 30 项通过；仅出现本次未涉及的 orchestrator 既有 dead-code 警告。Rust 格式与 diff 空白检查通过。未调用真实 WeTest 构建、推送或部署，也未把提示词契约测试表述为平台端到端验证。

## task 配置复用与每次 run 确认（2026-09-11）

根因：task `memory.json` 允许在同一 task 下复用，但原提示词只要求使用已有配置预填，未明确每次 run 必须重新确认，也保留了同范围确认可直接沿用的表述。子系统、应用、分支、环境或模板变化时，模型可能把上一次 run 的配置误当成本次触发授权。

- [x] 最小失败测试：新增中英文契约断言，旧正文缺少“每次 run 都必须重新确认构建和部署参数”及“不能复用上一次 run 的确认”，测试稳定失败。
- [x] task 配置降级为本次 run 的预填来源；每次 run 在构建、推送、部署前必须展示最终生效范围并由用户确认，用户可以调整参数，未确认前只允许查询和准备。
- [x] 当前 run 的构建、推送和部署确认与历史 run 隔离；只有当前 run 已确认的连续查询或等待可以继续，不得用历史确认跳过新的触发确认。
- [x] 同步中英文提示词、产品设计文档和本开发计划，并使用同一契约测试转绿。

过度设计审查：复用现有 task 文件和交互能力，只收紧提示词授权契约，不新增确认状态机、持久字段或执行队列。性能审查：每次 run 增加一次有限的参数展示与确认，不增加文件扫描、网络查询、轮询或后台任务；配置仍按所选子系统数量线性处理。

验收：`wb` 编译条件下新增确认契约测试通过；`default` / `wb` 两组完整 Profile 测试各 30 项通过，Rust 格式与 diff 空白检查通过。仅出现本次未涉及的 orchestrator 既有 dead-code 警告。未执行真实 WeTest 写操作。

## 渠道单一真源与内置只读覆盖（2026-09-11）

根因分析：渠道事实原先由 core crate 与桌面 crate 各自解析同一个编译期变量，权威校验只存在于 `src-tauri/build.rs`，两侧派生逻辑一旦分叉，客户端渠道身份与内置能力目录就会不一致；同时内置角色只读保护的唯一断言位于 CI/CD 的 wb 专属分支内并在非 wb 渠道提前返回，`default` 渠道（含 CI）因此不再校验内置角色不可改、不可删。两类问题都属于“好设计但实现不完整”，所以在同一机制上补齐真源与覆盖，不新增第二套渠道模型。

- [x] 新增 core crate `src/channel.rs`，提供 `RELEASE_CHANNEL` 与 `WB_CHANNEL` 常量；`src/app/profiles.rs` 的可用目录过滤与 seed 渠道标注、`src-tauri/src/channel.rs` 的桌面渠道身份统一引用该常量，同一事实只保留一个读取点。
- [x] 保留 `src-tauri/build.rs` 对 `configs/channels/<channel>.json` 的校验与 `cargo:rustc-env` 注入，不新增渠道来源。
- [x] 新增跨 crate 一致性断言 `desktop_channel_matches_core_release_channel`，固定“构建脚本注入值等于 core crate 编译期常量”。
- [x] 新增渠道无关测试 `built_in_profiles_reject_update_and_delete_in_every_channel`，遍历当前渠道可见的全部内置角色，断言修改与删除均返回 `ReadonlyBuiltIn`。
- [x] 只读保护反证：临时把 update / delete 的内置守卫置为 false，新测试立即失败（`unwrap_err` 命中 `Ok`），证明该测试能拦截只读保护缺失；随后还原。
- [x] 跨 crate 断言反证：临时让构建脚本注入固定串，断言失败并给出 `left: "probe-mismatch"` / `right: "default"`；随后还原。
- [x] 验收：`default` 与 `wb` 各 31 项 Profile 测试通过（原 30 项加新增 1 项）；跨 crate 断言在两种渠道各 1 项通过；`cargo fmt --all` 与 `git diff --check` 通过。
- [x] 同步产品设计文档与本开发计划。未改动提示词正文、构建脚本产出、CI 工作流或对外设置。

过度设计审查：只新增一个常量模块与两条断言，复用现有渠道变量与既有接口，不新增依赖、配置项、缓存、状态机或运行时分支，也未触达审批与执行路径。性能审查：`RELEASE_CHANNEL` 是编译期常量，运行时零开销；目录过滤仍为 O(10)，新增测试只覆盖既有静态数据，不引入扫描、I/O、并发或重渲染。

验收环境说明：Profile 测试使用完整 Windows 临时路径，未修改持久环境变量；`wb` 验证通过设置 `GOLD_BAND_RELEASE_CHANNEL=wb` 完成，并确认 core crate 重新编译；未执行真实 WeTest 构建、推送或部署。

## 2026-09-15 默认判定调整

- [x] WB 默认 `wb-development-cicd` 模板的 CICD 节点改为人工 check，结果模式与 Plan 节点一致。
- [x] 移除默认 output、success_condition 与 cicd-result；会话自然结束后等待用户选择成功或失败。
- [x] 保留单次提交、人工恢复、禁止自动重放，以及自定义 CICD 节点显式配置 AI 输出验证时的 InlineControl 能力。
- [x] 更新双语提示词的最终控制结果边界，避免无 artifact 契约时自行推断成功。

验证：WB catalog、模板单测、CICD 输出契约、Profile 契约与恢复测试合计 7 项通过；Web 生产构建、Rust 格式检查和 diff 空白检查通过。未执行真实模型、WeTest 构建部署或 EXE 端到端操作。

自评审：只调整内置模板结果模式和静态提示词边界，复用现有人工 check、单次提交和人工恢复机制，不新增 runtime 状态、依赖、缓存、队列或迁移逻辑。默认模板保持常数级配置与有界执行成本；自定义 CICD 输出契约能力继续由独立接口测试覆盖。

## 2026-09-15 main 合并

- [x] 保留共享记忆 `memory_read / memory_write` 和默认 CICD 人工 check。
- [x] 吸收 main 的 `RELEASE_CHANNEL / WB_CHANNEL` 单一真源，CICD 仅 WB 渠道可用。
- [x] 吸收 main 的 task 级唯一构建与逐子系统部署规则：记忆 key 迁移为 `cicd.build.*` 与 `cicd.deploy.<S>.*`，不再使用每子系统 `cicd.<S>.build.*`。
- [x] 每次 run 重新确认最终构建和部署参数，记忆仅作为预填，历史确认不可复用。
- [x] CLI 参考更新为 0.2.12，构建产物覆盖面按后续只读查询和实际证据核实。

## PR #123 与 main 语义合并（2026-09-16）

根因：PR 的共享记忆实现沿用了每子系统 build key，而 main 已根据真实 Jenkins 生命周期将构建收敛为 task 级、部署保留子系统级；同时 PR 通过存储目录推导 WB，和 main 的编译期渠道真源形成分叉。这属于正确共享记忆设计与后续正确领域设计未接通，不保留双格式兼容层。

- [x] 渠道判断统一从 `src/channel.rs` 的 `RELEASE_CHANNEL` / `WB_CHANNEL` 派生，Profile 目录保留 main 的 `release_channel` 过滤器。
- [x] 双语角色改为 task 级 `cicd.build.jobId/branch/appList/appCoverage` 与子系统级 `cicd.deploy.<S>.*`，全部通过 `memory_read` / `memory_write` 和逐 key revision/CAS 维护。
- [x] 删除角色直接读写 task 配置文件及整份 JSON 模板的路径；参数只作预填，每次 run 重新展示并确认生效范围，上一次 run 的确认不可复用。
- [x] 最小失败测试在旧提示词上因缺少 `cicd.build.jobId` 失败；修改双语正文后同一 `cicd_profile_contract` 测试转绿。
- [x] 完成合并后的 Rust、前端、格式、构建与 UI 验收并补充最终结果。

自评审：只修正现有 key 投影与渠道消费点，没有新增持久字段、状态机、兼容层、缓存、队列或依赖。每次读取仍为两个最多各 100 条、有效合计 32 KiB 的文件，合并 O(P + T)；task 级 build 为常数条目，部署参数和必要平台查询随已选子系统数线性增长。锁不覆盖模型、用户交互或外部网络调用，无需新增 benchmark。

最终验收：WB 渠道 7 个 Rust 目标共 20 项通过；独立 target 的 default 渠道隔离 3 项通过；项目记忆与侧栏前端 2 个文件共 14 项通过，TypeScript、Vite 生产构建、Agent catalog 7 项和桌面端 `cargo check -j 1 -p gold-band-desktop` 通过。格式与 diff 空白检查通过；Cargo.lock、ACP registry snapshot 与 agent catalog 在业务语义合并后重新生成并校验。UI 按规则先尝试 iab 和已连接 Chrome，环境不可用后使用 agent-browser 回退，覆盖正常/窄宽度、长文本、明暗主题、未保存关闭确认和保存后重开；会话、浏览器与开发服务器已清理。未调用真实 WeTest 写操作。

渠道单一真源收紧：`MemoryService::new` 不再接受调用方渠道布尔值，Tauri、provider 和 MCP 只能使用 `src/channel.rs`；MCP 启动 JSON 不再携带 `wb`，并由接口测试固定该约束。该调整删除重复输入，没有增加运行时分支。性能复核仍为两个有界文件的 O(P + T) 合并，无历史扫描、N+1、无界缓存/队列或长时间持锁；MCP helper 仅有不随数据规模增长的固定进程启动开销。

## 2026-09-17 提交职责迁移与推送继续确认

根因：CICD 同时承担提交和推送，而代码生产者是开发测试节点，属于生命周期职责错位。未推送提交会直接影响远端构建输入，因此 push 不能随 commit 一起删除，但用户拒绝 push 或 push 失败后必须提供按远端现状继续构建的显式选择。

- [x] 双语 CICD 删除 `git add`、`git commit` 和提交消息职责，只检查当前分支是否存在未 push 提交并提醒用户是否 push。
- [x] 存在未推送提交时先询问是否 push；成功后直接继续。拒绝 push 或 push 失败时，说明远端缺失风险并询问“继续构建 / 停止”。
- [x] 用户选择继续时按远端现状进入构建和已确认的后续流程；选择停止时不触发构建。
- [x] CICD 参数固定写任务作用域：`cicd.build.*` 与 `cicd.deploy.<S>.*` 全部以 `scope=task` 写入；`subSysId*` 仍只在工作空间维护。
- [x] 参数写入后重新 `memory_read` 逐 key 核验；工具不可用、部分写入或核验失败时不再阻塞，向用户询问或确认本次参数后继续，并说明未持久化。
- [x] `cicd_profile_contract` 更新为禁止旧提交职责、固定推送继续分支和任务作用域核验。

验证：WB 渠道 `cicd_profile_contract` 1/1、`memory_domain` 13/13、`wb_workflow_profile_contract` 1/1 通过；`cargo check -p gold-band --tests -j 1`、`cargo fmt --all` 和 diff 空白检查通过。

自评审：复用现有 Profile、共享记忆和 Git CLI，不新增状态机、持久字段、依赖、缓存或队列。新增正文为固定长度，Git 只检查当前分支是否存在未 push 提交，记忆读取仍为两个有界文件的 O(P + T) 合并。
