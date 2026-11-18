# 首次启动引导技术方案

> 状态：2026-10-26 已补 Windows 真实客户端＋受控 ACP 的环境展示、主动重测、退出恢复、自动保存及标准版批量绑定验收；候选环境策略尚未通过全部供应商／平台的发布矩阵。\
> 调研日期：2026-10-16；范围更新：2026-11-09。范围：Gold Band 桌面客户端，优先验收 Windows，沿用现有跨平台进程边界。\
> 配套计划：[首次启动引导开发计划](../../../开发计划/新增流程/首次启动引导开发计划.md)。

> 后续 Catalog 扩展：[DeepSeek Harness / ZCode / Qoder ACP 接入技术方案](../../../开发计划/acp接入/DeepSeek-ZCode-Qoder-ACP接入技术方案.md) 不改变本文 onboarding 状态机。当前发布版消费 14 项，新增 DeepSeek Harness、Qoder 与 ZCode；Qoder 在 Windows aarch64 被平台过滤，不会成为候选。未准入或当前平台不支持的条目不会成为候选。

## 1. 推荐决策

保留“Git → Node.js/npm → Agent 与模型”三个步骤。首次使用完成后，用户获得可用的默认 Agent、经过能力校验的模型配置，以及已经完成本机模型绑定的全部内置工作流。

| 原始需求 | 优化后的设计 | 原因 |
| --- | --- | --- |
| Git 版本低时提醒，可跳过 | 检查安装、版本和可执行性；缺失、过低或无法判断均可显式跳过；运行 Git 相关功能时仍检查 | 跳过引导不等于绕过工作流执行前置条件 |
| Node.js/npm/npx 异常是否阻断 | 同页检查 Node.js、npm、npx 和组合兼容性，任何结果都不阻断引导，只按影响范围提示；npx 不能启动时跳过依赖 npx 的候选并说明原因 | 运行环境依赖由各 Agent 的启动方式决定，不是所有 ACP Agent 的硬依赖 |
| 检测可用 Agent 并选择模型 | 自动发现并有界诊断；全部诊断成功的 Agent 自动配置到 Agent 管理，第三步选择器只展示诊断成功且已保存的 Agent；模型来自实际 ACP 能力 | 用户只需选择默认 Agent 和模型，无需额外添加或从异常候选中筛选 |
| Agent 检测结束（无论是否有可用项）或检测失败 | 运行环境已通过时提供“稍后设置”，把首次引导转为 `deferred` 并进入主界面；不标记完成，不修改默认偏好和工作流，之后可从设置重新运行引导 | 允许用户先进入应用，避免首次引导成为必须立即完成的阻塞 |
| 更新所有默认工作流 | 按内置模板身份批量更新 `WorkflowModelBindings`，先预览再一次提交；保留工作流结构及权限语义 | 防止覆盖用户模板、运行快照和已有任务 |
| 第一次安装启动时展示 | 以当前用户配置空间的首次初始化为界；升级、重装保留配置不反复强制引导 | 安装包事件不是可靠的业务身份 |

2026-11-14 起运行环境不再是引导门槛：Node.js、npm、npx 的版本与组合检查只用于展示和修复建议，Agent 能否使用以 Doctor 结果为准。只有通过 npx 启动的候选依赖完整工具链；npx 无法启动（缺失、启动失败、超时）时这类候选直接跳过并返回 `runtime-required`，不启动进程。本机 ACP 命令与本机 CLI 不做版本前置判断，照常诊断。最终提交仍要求至少一个配置匹配且 Doctor 健康的 Agent。

## 2. 接入范围与环境版本策略

### 2.1 复用既有 Agent 接入

本期围绕现有精选 Catalog、ACP client、Doctor 和 Agent 管理能力完成首次启动引导，保留现有供应商条目与配置。供应商新增、替换、退役及对应设置迁移不属于本需求；后续新增条目只有在独立接入方案通过发布门槛并进入 Catalog 后，才由本文既有发现链路自动消费，不为供应商复制 onboarding 状态。

引导面向新安装客户端：发现来源是精选 Catalog 与当前电脑的本机程序，不是 Agent 管理中已配置的实例数量；即使 `SettingsConfig.agents` 为空也必须完成发现。既有配置只用于复用稳定 ID、用户字段和冲突基线，不能作为可用性证据。

Catalog 同时提供本机 ACP 命令与 npm 适配器启动信息。优先使用已安装的 ACP 命令；对已发现的 Claude、Codex、Pi CLI，缺少 ACP 适配器时自动衔接对应 Catalog 的受控 npx 启动流程。仅有启动器不算安装了 Agent，只有实际 ACP 诊断成功并保存的项才算可用；不自动下载电脑上没有安装的全部精选 Agent。

候选生成先调用后端统一的类型化平台判定，再进行本机发现。Catalog 平台值统一为 `windows/macos/linux` 与 `x86_64/aarch64`；Web 不解释 Node `win32/arm64` 字符串。平台不支持的内置条目不进入引导候选，直接绕过 UI 调用创建接口时仍由后端返回 `agent.unsupported-platform`。已经配置的实例不因后续限制被删除；引导继续只投影其匹配的健康诊断。

### 2.2 版本建议与事实区分

| 项目 | 本期建议策略 | 依据及限制 |
| --- | --- | --- |
| Git | 最低 `2.36.0` | 复用 `src/git.rs::MINIMUM_SUPPORTED_GIT_VERSION`，这是项目能力门槛，不代表该旧版本仍获官方安全维护 |
| Node.js | 已验证基线为 `>=22.20.0 <23` 或 `>=24.0.0 <25`；不属于已验证 major 的稳定版本在实际工具链兼容时给出警告并允许继续 | 22、24 是调研日的 LTS 系列；采用 22.20 起点参考其 OpenSSL 升级，最终补丁下限由发布兼容矩阵收紧；Node 23.11.0 等存量环境不因未进入验证矩阵而被强制阻断 |
| npm | 初版候选最低 `10.9.0`，验证 10.x／11.x／12.x | 同时检查实际 npm 的 Node.js engines，以及所选 npm ACP 包的 engines；此数值是产品候选基线，并非上游统一要求。npm 12 由 engines 限定 Node `^22.22.2` 或 `^24.15.0` 以上，组合不满足时按 engines 不兼容报告 |
| 推荐新装 | Node.js 24 LTS 与其配套 npm | 调研日官方索引列出 `24.22.0`，附带 npm `11.19.1`；发版重新核实，不把“最新版”写进业务逻辑 |
| npx | 必须存在且可启动 | 检查与选定 Node/npm 的执行环境一致，不通过运行远程包做版本探测 |

来源 S1、S2、S3。候选门槛只有在支持的 OS／架构与精选 npm ACP 包组合通过 CI 后才转为发布策略。不能把本仓库构建工具的 Node.js engines 当成 EXE 用户的运行要求。

版本策略集中在打包的 `configs/onboarding-environment-policy.json`，由状态 IPC 返回只读摘要；Git 门槛引用现有 Git 领域常量，不复制第二份。包含已验证的 major 范围、最低版本、推荐安装版本、官方帮助目标和策略版本。离线检测使用打包策略，联网与否不改变版本比较结果。已知且稳定但落在验证范围外的新 major 显示“未验证版本”，仍需完成实际工具链、路径和 npm engines 交叉检查；交叉检查通过时作为非阻断警告继续，不把它静默标成已验证。

版本比较复用已有 Rust `semver`，不按字符串比较；正式门槛不接受预发布版本。npm engines 的范围语法在构建兼容矩阵时使用 npm 生态的标准解析器处理，不能假定它与 Rust 版本范围语法完全相同。安装器下载成功不等于检测成功，必须重新解析实际使用的本机工具链。

2026-11-04 npm 12 策略复核：npm 12.2.1 是官方 `latest`，engines 为 `^22.22.2 || ^24.15.0 || >=26.0.0`，`npx -y` 仍可用且 `_npx` 缓存布局未变，因此加入已验证 major 窗口。窗口只声明 npx 行为契约，不承担 Node 兼容性判断；Node 组合兼容仍由实际 npm 包的 engines 与实际 Node 版本交叉校验。不改成单纯的 `>=10.9` 下限，因为未验证的更高 major 会在发布第一天被静默接受，而 Agent 启动依赖 npx 的 exec 语义。

2026-11-04 版本状态区分：`EnvironmentStatus` 增加 `unverified`，与 `unsupported` 分离。低于最小已验证 major、落在已验证 major 的接收区间外或预发布版本仍为 `unsupported`；高于或不属于任何已验证 major 的稳定版本为 `unverified`，返回 `setup.runtime-version-unverified`。2026-11-05 起，`unverified` 不再单独阻断 `runtimeReady`：只有当实际 Node/npm/npx 探针、路径一致性和 npm engines 交叉检查也通过时，才允许继续并在界面显示警告；过低、预发布、缺失、启动失败、路径不一致或 engines 不兼容仍硬阻断。`verifiedMajors` 与 `ranges` 同属打包策略，并以测试固定每个区间的下界 major 已登记。

2026-11-14 版本策略降级为提示：上述 `ready / unverified / unsupported` 分级、路径一致性与 npm engines 交叉检查保持不变，但任何结果都不再阻断引导。已验证区间表示“已测试”，不是“只有这些版本能运行”；引导只诊断已准备的本机包，不触发下载，Doctor 是唯一可用性证据。npm/npx 检查额外上报其进程实际使用的 Node.js 版本与路径（`nodeVersion`、`nodeExecutablePath`），因为 npx 启动的 Agent 运行在该 Node 上。通过 npx 启动的候选 Doctor 失败且 Node.js 版本为 `unverified / unsupported` 时，失败行附带当前版本与推荐版本的提示；其他运行环境异常附带通用提示与“查看运行环境”。

## 3. 现状、根因与复用边界

实施核验补充（2026-10-21）：兼容矩阵脚本已覆盖当前 Catalog 全部 6 个精选 npm 包（含 Gemini）、npm 候选版本与 6 个 OS／CPU 组合，并核验官方 Node 发布索引。未声明 engines 的项保留 unknown，全部平台运行结果仍为 not-tested；元数据通过不等同于安装、认证或实际 ACP 可用。

### 3.1 现有能力

| 领域 | 当前实现 | 本次用途 |
| --- | --- | --- |
| Agent 模板 | `scripts/prepare-agent-catalog.mjs`、`src/agent_catalog.rs`、`resources/agent-catalog.json` | 复用现有 14 类精选模板及构建期校验，作为安装发现与准备指引的来源 |
| Agent 实例 | `SettingsConfig.agents`、`ManagedAgentId`、`ManagedAgentConfig` | 唯一实例事实源；不增加 onboarding 专用 Agent 表 |
| Catalog 启动配置 | `src/config/managed_agents.rs` | 当前代码保存时剔除内置 command/args，读取时从 Catalog 注入；设计必须遵循实际代码 |
| 诊断 | `src/acp/client.rs::doctor_with_deadline`、`probe_model_config` | 复用 ACP 会话建立、模型能力探测及清理；不重做健康检查 |
| Git 检测 | `src/git.rs` | 复用版本解析和可执行文件刷新，抽出不依赖仓库的环境检测接口 |
| 进程管理 | `src/process.rs` | 复用 PATH 解析、Windows 隐藏窗口、进程树回收 |
| 模型绑定 | `src/workflow_model_binding.rs` | 复用 slot、definition revision、binding revision、规范化及注入校验 |
| 模板存储 | `src/app/mod.rs` 的 `WorkflowTemplateStore` | 默认完整、轻量及启用渠道的内置模板统一处理 |
| 持久化 | `src/storage/mod.rs` | 复用单文件原子写；补齐跨文件提交边界 |
| UI | 现有 shadcn/ui Button、Select、Command、Progress、Alert 等 | 独立引导页组合已有控件，不自研向导控件库 |

`agent-management.md` 已收敛到当前序列化契约：Catalog 管理内置 command/args，实例保留 env、目录、图标与可选 executablePath；发现、普通运行和 Doctor 共用 executable_adapter。

### 3.2 根因判断

1. **首次使用缺少全局协调入口**：环境、Agent 诊断、模型绑定各自成立，但用户需要自行连接这些步骤。补齐一个薄的引导应用服务，保留领域逻辑。
2. **默认工作流初值与本机选择缺少衔接**：已删除构造器中无实际消费的固定 `claude-acp` 参数。绑定分层保留；执行 slot 是完整枚举来源，即使初始 bindings 为空，也逐槽补齐并按既有顺序规范化。
3. **已安装状态不足以证明可用**：发现结果需要进入既有 ACP 诊断链路，按实际能力与认证状态判定，不以 CLI 存在或品牌名称推断可用性。
4. **跨文件“完成”没有统一提交契约**：设置与工作流分别原子写，仍可能出现只更新一半。需要范围明确的提交恢复，而非失败后随意重跑全部写操作。
5. **发现、配置与选择不应成为三次用户操作**：诊断成功已足以触发候选正式配置，用户只决定默认偏好。修正成功后的持久化与列表投影顺序，复用 Agent 管理事实源，不另设向导专用 Agent 列表。
6. **引导与工作流配置表单分叉**：直接把 ACP 参数目录逐项渲染，会暴露无关供应商配置，并使模型切换与思考强度恢复规则不一致。引导复用工作流的模型／思考及权限控件和参数映射，只保留首次默认配置所需的选择。

## 4. 用户流程

### 4.1 入口与恢复

- 引导为独立页面 `#/onboarding`，在 workspace 选择前可用；`#/onboarding/defaults` 用于已有用户直接预览默认配置应用。首屏仅加载状态与静态策略，业务 React App 延迟挂载；首次 pending 时延后业务恢复、搜索索引、远程服务与调度启动，提交成功后只启动一次。
- 初始化在任何默认配置 seeding 之前判断是否已有用户数据，并写入一次性的 onboarding 状态。不能在默认文件已生成后仅用“settings.json 是否存在”判断首次使用。
- 新用户进入引导；中途关闭客户端后重新进入并重测环境；已完成用户正常进入主界面。
- Agent 步骤在运行环境已通过且本轮检测结束（无论是否有可用 Agent）或检测失败时提供“稍后设置”；该动作将 `pending` 转为 `deferred` 并进入主界面。即使已有可用 Agent，defer 也不会自动选择它；deferred 不等于 completed：不设置默认 Agent、模型或工作流绑定，用户之后可从设置重新运行引导。
- 引导、加载状态与业务页面共用应用入口的窗口关闭协调器。关闭窗口或退出应用时先保存文件草稿；保存失败沿用“重试／取消／丢弃”确认。取消保留未保存内容与当前引导检测，页面切换不得卸载关闭保护或中断待处理的确认。平台值随轻量启动状态返回，不为关闭保护提前加载业务 App。
- 已有旧版用户标记为 `existing-install`，提供设置中的主动入口；不因新增字段缺失而把所有老用户拦在 npm 步骤。
- 设置页的主动入口纳入“通用”卡片内的“引导与工作流”标准分区，位于浏览器设置之后，与其他设置共用标题列、内容列、留白及分隔线；不在卡片外另放操作横栏。“首次启动引导”与“默认工作流”分别占一行，提供简短用途说明和紧凑描边按钮。后者标为“配置默认项”，说明仅调整内置工作流、不影响自定义工作流；点击只进入原配置流程，不能直接应用。窄窗口和长译文按内容宽度换行，不新增嵌套卡片或独立滚动区。
- 重装保留配置、客户端正常升级不触发重新初始化。换电脑携带配置后由当前运行前置检查发现环境变化，并提供重新配置入口。
- 引导期间可访问语言／主题、帮助和退出；业务启动入口不能绕过引导。受控例外只有 Agent 步骤的“稍后设置”：其前置条件是本次运行环境报告已通过，写入 `deferred` 后才允许挂载业务 App。本版本不提供外部 deep link 直接启动业务页面的入口，也不承诺完成引导后恢复启动前 hash；完成后进入 workspace 选择或默认业务入口。
- 尚无 workspace 时，Doctor 使用应用管理的隔离临时目录，不执行项目脚本、不初始化用户仓库；首次选择真实 workspace 后按正常运行规则再次预检。

### 4.2 第一步：检查 Git（允许跳过）

显示检测状态、已安装版本及最低版本。未安装、版本过低、不可启动、解析失败、超时分别保留原因，提供“安装／升级 Git”“重新检测”“暂时跳过”。版本合格显示“下一步”，不自动跳页。

跳过时同页简短提示“依赖 Git 的操作需要安装或升级后才能使用”，不额外叠加确认弹窗。不将没有 workspace、没有 HEAD 或没有 worktree 判为 Git 安装失败。

跳过只记录用户选择，不制造 `ready` 健康状态。后续仓库、worktree、提交等操作继续由 Git 领域检查实际能力。

### 4.3 第二步：检查运行环境

页标题“检查运行环境”，说明“部分 Agent 通过 npx 启动，需要 Node.js、npm 和 npx。其他 Agent 不受影响。”。同页展示 Node.js、npm、npx 三项结果；npm/npx 行额外展示其进程实际使用的 Node.js 版本与路径。

- 全部符合策略：步骤条显示完成，允许下一步。
- 任一项缺失、版本未验证或不支持、启动失败、超时或组合不一致：该项以警告色展示状态与原因，步骤条显示警告，仍允许下一步。红色只用于 Git 步骤等真正阻断的结果。
- npx 无法启动（`npxLaunchable=false`）：额外提示“通过 npx 启动的 Agent 暂时无法使用，其他 Agent 可继续检测。”。
- 环境检测本身未完成（检测中、请求失败或已取消）：不可进入下一步，提供“重新检测”。
- 无网络但本地环境合格：可以继续；不能以下载站不可达判定本地环境不合格。
- 超时、无法解析版本与执行失败不能当作“未安装”或检测成功；版本、路径和 engines 的细分原因保持原样展示，作为修复依据。

第二步检测完成后，由后端现有环境请求保留一份本次报告，进入第三步、返回步骤、刷新 Agent、加载下一批和最终应用均复用该报告，不再启动环境版本命令。后端只要求报告已完成且未取消；报告中的 `npxLaunchable` 随检测批次固化，决定 npx 启动的候选是否诊断。不能接收前端自报的环境状态。默认配置直达入口同样先进行一次初始环境检测。

只有首次进入引导或用户主动点击“重新检测”才启动环境命令；安装、升级或切换工具后由用户主动重测。重测开始立即替换旧报告，失败不得回用上次通过结果。报告保留 Node.js／npm／npx 的状态、版本、命令路径、实际 Node 路径及结构化原因；启动命令路径只表示实际被调用的启动器，Windows `.cmd`、版本管理器 shim 或符号链接可以与真实 `node.exe` 路径不同，联合一致性以三次探针实际观测到的 `process.execPath` 为准。超时保留为超时，组合不一致展示实际差异，不归类为未安装。退出引导、页面卸载或客户端关闭时释放报告；下次进入重新检测，不设置 TTL、后台轮询或持久化健康标志。Agent 既有配置的启动失败由 Agent 管理页展示并重新诊断，引导仅隐藏不可用项，不自动重跑整套环境检查。用户随时可以退出，退出不等于完成引导。

本期沿用现有非托管安装边界：由按钮打开官方安装页面／系统安装流程，安装后重新检测；不默认执行 `npm install -g npm@latest`，不自动更改全局 npm registry、代理或用户的 Node 版本管理器。帮助页走现有 `openWebTarget`；OAuth 仅在供应商流程需要时使用系统浏览器。

### 4.4 第三步：选择 Agent、模型并应用

按以下顺序在同一页面渐进呈现：

1. 进入第三步从当前电脑发现本机候选；未配置候选通过既有 Doctor 有界调度诊断，已配置候选读取 Agent 管理领域按 fingerprint 校验的 canonical 健康快照。不要求 Agent 管理预先配置新候选，页面先显示检测进度，不要求用户先选候选再诊断。
2. 未配置候选诊断成功后，立即自动保存为正式 Agent 配置，并保存匹配该配置的诊断结果；Agent 管理页同步显示为已配置。全部新成功项均自动配置，不限于最终选中的默认 Agent；既有健康项不重复保存或诊断。
3. 第三步 Agent 选择器只展示“诊断成功且配置已保存”的 Agent，不展示待诊断、诊断中、需登录、失败或未安装项，也不把它们作为禁用选项。成功一项即可渐进展示和选择，无需等待整批检测完成。
4. 用户选择默认 Agent、模型、思考强度和权限模式；模型／思考配置复用工作流的 `AcpModelThoughtSelects`，权限复用 `AcpSingleConfigMenu`。默认 Agent 与模型相关配置会成为后续新建配置的初值；权限模式只用于生成本次内置工作流绑定，不进入后续新建会话的默认执行偏好。可配置项及模型绑定规则与工作流一致，只呈现实际支持的模型相关配置，不直接展开其他供应商通用参数。此处不提供“添加 Agent”或重复“连接”动作。
5. 页面底部展示将更新的内置模板名称、模板数和执行 slot 数，所选 Agent、模型与权限保留在上方表单。
6. 点击“应用配置并开始使用”，后端校验并提交；成功后进入 workspace 选择或默认业务入口。

状态行分三种：有可用项时显示“检测到 N 个可用 Agent”；正常结束只有不可用项时显示“检测到的 Agent 暂不可用，请查看下方原因。”；正常结束什么都没检测到时才显示“未检测到已安装的 Agent。安装任一 Agent 后重新检测。”。

选择器下方的“N 个 Agent 暂不可用”折叠区（shadcn/ui `Collapsible`）只列本机已安装但不能用的 Agent，按结构化原因展示：
- `runtime-required`：显示“需要 npm/npx”和“查看运行环境”，返回第二步展示同一份报告，不隐式重测。
- `doctor-failed`：显示“诊断未通过：{{reason}}”，reason 复用 Agent 管理页的精简原因规则（原始报错首行，120 字以内）；ⓘ Popover 展示完整原因。通过 npx 启动且运行环境有警告时附带版本或环境提示与“查看运行环境”。
- `not-installed`：不展示，避免列出全部目录条目。

折叠区默认在没有可用 Agent 时展开、有可用 Agent 时收起；用户手动展开或收起后保持用户选择。修复已配置实例仍在 Agent 管理页完成，引导不提供重试单项、程序选择或自定义 ACP 配置入口。完成本轮检测后（无论是否有可用 Agent）或检测失败时，本步骤提供“稍后设置”：用户可先进入主界面，之后从设置重新运行引导；即使已有可用 Agent，该动作也不自动选择 Agent，不修改默认偏好或内置工作流。

第三步直接基于本次已完成的环境报告进入 Agent 检测，不增加环境复验等待阶段。默认配置直达入口的初始环境检测、Agent 检测失败、检测中和正常结束后的空结果分别呈现，不能从空列表推断“无可用 Agent”。若后端环境报告已被释放或取消（`setup.environment-check-required`），撤销预览和应用请求、禁用提交，状态行显示“运行环境检测未通过”并提供“重新检测运行环境”，保留 Agent／模型／思考强度／权限草稿；重测完成后自动继续 Agent，重新预览再提交，应用动作本身不启动环境命令。

每轮完整检测对未配置候选验证当前本机可用性；已配置 Agent 仅在配置 fingerprint 匹配且 canonical Doctor 健康时直接复用。若上次 Doctor 成功但 fingerprint 已缺失或变化，说明该结果属于旧配置：引导对该已配置 Agent 执行一次受控重诊断，成功后更新健康快照并展示；失败只隐藏本轮结果，不覆盖管理页已有快照，也不把 transient 失败写回管理页。重新检测或返回第三步时先清空上一轮可选结果并禁用提交，保留所选 ID 和模型草稿；同一 Agent 重新变为健康后恢复选择。追加下一批时保留此前已验证结果。重复检测复用正式实例，不产生副本；不可用项不进入本轮可选列表。Agent 管理页继续保留已配置项及其异常状态，不能因第三步隐藏而自动删除配置。

检测阶段只执行只读发现、诊断和成功项自动登记，不创建、迁移或升级工作流模板，也不初始化模板默认绑定。全新安装或待升级模板在预览时生成内存投影；只有最终应用在同一提交边界内写入 settings 与 workflows。取消检测保留已登记的成功 Agent，但不改变模板文件。

步骤条表达 Git、运行环境和 Agent 检测的生命周期；模型与权限属于配置完成度，由底部预览／应用按钮的可用状态表达。Agent 检测正常结束后，即使尚未选择模型或权限，Agent 步骤也显示检测完成；底部预览和应用仍要求所选 Agent、模型与权限满足提交条件。

自动配置是成功诊断后的独立成果，不等待最后“应用配置并开始使用”。用户返回上一步或退出引导时保留已自动配置的 Agent；默认 Agent、模型和工作流绑定仍只在最后确认时更新。

不预选付费模型、不创建付费探测 prompt、不擅自扩大权限。若需要真实推理验证，作为单独且明确说明可能消耗额度的可选操作，不成为强制门槛。

### 4.5 默认配置的含义

“设为默认”有两个明确效果：

- 为没有显式选择的新建会话／新建执行配置提供默认 Agent 与模型相关配置初值；权限模式不进入该默认值。显式选择和已有运行快照优先，不在运行时静默 fallback。
- 本次确认后使用用户选择的模型与权限模式更新全部当前渠道内置工作流的模型绑定。

以后用户在设置中仅修改默认 Agent，不自动重写已有模板；提供独立“应用到内置工作流”动作并预览。Agent 被删除、凭据失效或模型下线后保留失效引用并提示修复，不自动换成列表中的第一个 Agent。

## 5. Agent Catalog、安装发现与认证

### 5.1 既有 Catalog 与本机程序定位

- 沿用现有精选 ID、启动配置、图标与构建完整性校验；引导只消费已支持条目。
- 内置 `command/args` 仍由 Catalog 决定；新增可选的 `executablePath` 属于 Agent 实例本机位置，只覆盖程序定位，不允许拼接额外参数。默认按 PATH 与已确认安装位置查找。
- `executablePath` 必须由后端路径校验；运行、Doctor、发现共同使用同一解析函数。此字段用于定位已安装但不在 PATH 中的程序，不引入第二 Agent ID。
- Catalog 的 `localLaunch` 保存已核实的本机 ACP 命令与 ACP 参数，`installedCommand` 保存需要单独适配器的底层 CLI 命令；这些是静态模板元数据，不增加实例持久字段。发现与普通运行共用 `executable_adapter`，优先采用本机 ACP 命令及正确参数，不将 npx 的包名、`-y` 传给本机 CLI。显式 `executablePath` 仍优先，但它只替换 `localLaunch.command` 的程序位置，必须继续使用 `localLaunch.args`；条目没有 `localLaunch` 时才保留原 Catalog 参数。
- Skills 根目录与兼容目录沿用既有 Catalog 中已验证的配置，不根据底层 CLI 的目录推断适配组件目录。
- Catalog entry 的 `sourceKind`、`integrationOrigin` 与 `validatedVersion` 只用于来源、信任和验证范围，不代替当前本机能力。社区桥接器只有通过独立真实后端门槛后才进入发布 Catalog；引导不把 candidate 或未验证条目展示给用户。

### 5.2 安装发现是只读的

发现优先级：已保存的有效路径 → 使用统一子进程 PATH 解析 → Catalog 中已确认的少量固定安装位置 → 用户文件选择。

- 不递归扫描用户目录或磁盘，不扫描历史会话，不读取凭据正文。
- Windows 使用现有注册表 PATH 刷新能力，识别 `.exe`／`.cmd` 与 npm shim；不能依赖 GUI 启动时继承的旧 PATH。
- 多个版本并存时展示实际采用路径；选定路径与正式执行路径必须相同，必要时允许用户选择其他安装。
- Node/npm/npx 不要求物理位于同一目录，但必须验证实际解析出的运行组合；不能把 A 安装的 Node 结果配上 B 安装的 npm。
- `npx` 模板可运行不等于包已安装。只读发现检查本机 ACP 命令、已准备的包及对应底层 CLI，不因配置列表非空或 npx 存在而判定可用。
- 已准备的 npm 包按目标精确 spec 的确定缓存目录、根依赖声明、安装锁记录、包名／版本和包内启动脚本共同校验；不得要求 npm 未保证存在的私有元数据。只定点读取对应缓存，不遍历所有版本；缺失安装锁、身份或版本不一致时不得将包视为已准备。
- 已安装的本机 ACP 命令直接进入诊断。只有发现对应底层 CLI，才允许在后续受控 Doctor 中按既有 Catalog 命令补齐该 Agent 的适配器；安装、握手、会话建立与清理仍由同一取消标志、并发预算和 deadline 管理。只读定位本身不下载，不为填充列表下载未安装 Agent；下载或认证失败均不发布 ready。
- 所有无需交互的命令通过 `process::background_command()` 创建，复用有截止时间的进程组；检测取消后回收自己创建的进程树。

### 5.3 认证与诊断契约

ACP client 读取 `initialize.authMethods`，与 `agentCapabilities` 一起保存在共享连接的初始化结果中；首次初始化、并发等待和后续复用连接的运行实例都取得同一份结果，不重复 initialize。认证同样按连接 single-flight：同一连接不会并发执行重复 `authenticate`；首个实例成功后，等待者直接重试 `session/new`，失败则释放等待者按各自错误路径收敛。`session/new` 返回结构化 AuthRequired 时，优先使用已有全部必填环境变量的 `env_var` 方法执行一次 `authenticate`，成功后只重试一次 `session/new`；ACP `optional` 变量缺失不影响方法可用性，也不进入 `missingEnv`。没有可用协议方法时（例如仅有缺少必填环境变量的 `env_var` 或 `terminal` 方法）返回 `provider.auth-required`，保留方法 ID 和缺失环境变量名；不能向 terminal 方法直接发送 authenticate。只有认证 RPC 的结构化失败转换为 `provider.auth-required`；Doctor 超时、用户取消、transport 中断与 adapter 退出保持原错误码。来源 S4。

自动选择严格限于显式 `env_var` 且必填变量齐全的方法，不回退到未声明类型、`agent`、`terminal` 或未知未来类型。方法 ID 和名称不能证明该方法无交互副作用；这些方法返回 `provider.auth-required`，周期检测不得通过 authenticate 自动打开供应商登录浏览器或终端。此约束同样适用于模型探测和普通会话，避免其他入口绕开边界。

供应商账号登录仍由其官方流程处理，完成后重新诊断；客户端不新增账号、token 存储或内嵌登录界面。检测页不恢复准备指引或自定义 ACP 折叠区。

本期主路径为官方图形／浏览器准备指引，登录凭据由供应商处理。若 Agent 仅支持终端登录，属于供应商的外部准备步骤；向导不新建终端产品界面，不宣称支持尚未实现的 auth capability。

引导不收集或保存供应商 token。若用户在 Agent 管理页显式使用 env 配置凭据，该 env 属于正式 Agent 配置并会保存到本机 settings；引导只复用配置指纹匹配的健康结果，不读取、展示或复制凭据正文。Qoder 等支持供应商登录的 Agent 优先使用其登录流程。

统一候选处理顺序：未配置候选走 `discovered → checking → 保存正式配置与匹配诊断 → ready`；已配置候选先按稳定 Agent ID 与配置 fingerprint 读取 Agent 管理领域的 canonical Doctor 快照，只有 `available=true` 且 fingerprint 匹配才直接投影为 `ready`。已配置诊断缺失、失败或过期时不在引导中展示，也不由引导再次写入失败快照；用户在 Agent 管理页重新诊断后，下一次引导再消费新的 canonical 结果。认证缺失、协议不支持、诊断失败或保存失败均不进入可选列表。`ready` 直接由正式 Agent 配置和匹配的成功诊断投影，不新增持久化注册状态机。`missing` 与 `checking` 不混用；模型目录成功不保证未来额度充足，页面称“连接正常”，不承诺一定能推理。

复用现有每 Agent 互斥、全应用最多 4 个 Doctor、单次 3 分钟总截止时间。未配置候选的自动诊断在进入第三步时启动，与用户选择解耦，逐项完成、保存和展示；已配置候选只读取 canonical Doctor 快照。有可用项即允许继续。认证由供应商或 Agent 管理页的独立用户操作完成，重新诊断后再次运行引导；不把等待用户输入算成已通过诊断。

### 5.4 诊断成功即自动配置

- 自动配置属于 Agent 管理领域，写入唯一的 `SettingsConfig.agents`；按稳定 `ManagedAgentId` 幂等登记，不按显示名称创建新实例。
- 已配置 ID 不由引导重复启动 Doctor；引导只复用 Agent 管理领域中与当前配置 fingerprint 匹配且健康的诊断快照，不覆盖用户的路径、环境变量、目录、图标等字段。快照缺失、失败或过期时从本轮选择器隐藏，用户需在 Agent 管理页显式重新诊断；相同 ID 的另一安装候选不得自动替换既有配置。
- 新候选在诊断前保持临时执行配置；成功后，在既有配置短时提交边界内重新校验探测 generation 与配置指纹，合并最新 settings 并原子保存，再持久化对应的诊断结果，最后发布可选项及 Agent 管理更新事件。
- 诊断成功结果直接用于此次自动配置，不套用普通人工编辑的“保存后清空诊断并再诊断一次”链路；复用持久化能力但区分写入意图，避免重复启动适配器。
- 配置或诊断持久化失败时不发布 ready；配置已写而诊断未写的中断状态可以恢复，重启后视为已配置但未确认可用，重新诊断即可，不需要用户重复添加。
- 诊断期间用户修改、删除实例或主动取消本轮处理，旧结果不得覆盖修改或重新创建已删除实例。重新检测属于新的显式意图；配置提交后的页面离开不回滚已保存项。
- 一个候选失败不回滚其他已配置成功项。自动配置只触及该 Agent，不设置默认偏好，也不修改工作流。

## 6. 模型选择与全部默认工作流更新

### 6.1 配置结构

新增 `DefaultExecutionProfile` 只保存用户的默认执行偏好：

```text
DefaultExecutionProfile
  agentId: ManagedAgentId
  modelSelection: explicit { modelId } | provider-default
  configOptions: Map<ConfigOptionId, String>
  modelBoundOverrides: Map<ModelId, Map<ConfigOptionId, String>>
```

使用显式 `provider-default` 表达供应商默认值，不能让 `null` 同时表示“尚未选择”和“使用默认”。写入既有 `WorkerModelBinding` 时按既有语义映射，不另建一套绑定模型。

结构沿用现有模型绑定契约；引导通过工作流共用组件和 `acpCompositeConfigSections` 投影当前模型的 `thought_level`／`model_config` 配置，权限模式由独立字段提交。未显式选择的参数使用供应商默认值；预览／应用仅提交当前模型对应的可配置项覆盖，不转提交未展示的通用参数或其他模型的旧覆盖值。

- 模型 ID、思考强度与权限模式来源于诊断及 `probe_model_config` 的实际能力，不写死 Gemini／Claude／GPT 模型名。引导沿用自身 generation 内的模型探测，不触发依赖 workspace 的普通模型发现请求。
- 切换 Agent 清理旧 Agent 的模型选择及参数；切换模型使用工作流同一 `switchAcpModelBoundOverrides`，按模型保留草稿、恢复已选思考强度，并根据实际能力重新验证。
- 优先使用标准 ACP `configOptions`，兼容当前 client 已支持的旧模型目录形态。来源 S5。
- 无模型列表但支持供应商默认值时显示“由 Agent 决定”；明确不支持模型切换时，不提供假下拉框。
- 查询失败与合法无列表分别处理。失败阻止提交，不把失败回退成 provider-default。
- 默认模型配置不包含默认 Auto Accept；首次初始化保持 `false`。批量换 Agent 不继承旧 provider 的权限字符串，必须选定并验证目标支持的安全权限；同 Agent 已显式配置的权限保持不变。
- “安全权限”不能通过名字猜测。目标 Agent 未提供可判断的默认值时，在主表单要求用户显式选择权限模式；不能为了完成引导开启无条件自动批准。

### 6.2 更新范围

“全部默认工作流”定义为后端内置模板集合：`is_built_in` 与当前版本／渠道的内置 ID 一致。当前标准版包括 `default`、`default-lightweight`；WB 渠道同时包括 `wb-development-cicd`。模板清单从现有构造器统一得到，不在页面硬编码数量或通过名称匹配。

| 对象 | 本次应用行为 |
| --- | --- |
| 内置模板普通 Worker、验证职责 Worker、可选入口节点 | 按现有 execution slot 更新绑定，即使可选节点当前未启用 |
| 工作流结构、边、Profile、角色提示词、输出协议、success condition | 保持原定义 |
| 用户创建／复制的模板 | 不更新 |
| 已有 Task authoring、运行中及历史 Run snapshot、定时任务冻结配置 | 不更新 |
| Direct／AUTO／AI-DYNAMIC 已保存配置 | 不批量覆盖；新建配置无显式选择时才读取默认 Agent 与模型相关初值，不读取引导权限模式 |
| 未来版本新增内置模板或 slot | 在显式模板初始化／升级阶段使用当时默认 Agent 与模型相关初值，仅填缺失，不重写已有绑定 |

当前内置模板按 Worker slot 绑定。未来若内置模板出现不经过这些 slot 的 AI-DYNAMIC 固定／bootstrap／availableAgents 配置，必须先扩展同一个模板绑定契约及测试；不能声称“全部成功”却漏掉执行节点，也不能在本轮为假设结构预建复杂映射。

用户再次运行引导时，预览必须列出内置模板里即将被覆盖的现有模型选择。确认后的覆盖范围可包括用户曾改过的内置模型绑定，但不得影响用户自建模板。

### 6.3 后端批量算法

1. 读取已初始化的模板快照与当前设置，筛选内置集合；普通 preview 读取不执行模板迁移／写回。
2. 收集模板稳定 ID、`definition_revision`、`binding_revision` 和全部 Worker execution slot；不以已有 bindings 数量代替槽位枚举。
3. 将同一默认模型选择及当前模型相关参数投影到所有目标 slot；完整替换目标绑定原有参数，不合并无关供应商配置；共享模型绑定校验继续拒绝非法配置。
4. 顺序执行“定义结构校验 → 注入本机绑定 → 可执行 snapshot 校验”；仍缺少能力的节点阻止整个提交。
5. 生成预览摘要与基于相关默认偏好、所选 Agent 配置、策略、能力目录和模板 revision 的预览指纹；不包含无关 Agent 的登记变化。
6. 提交前比对指纹，批量更新绑定 revision，一次写入完整模板存储；不逐模板调用现有单模板更新 IPC。

默认模型选择只影响模型相关字段。完整的参数对象必须替换旧 Agent 的对应参数，不对不同 Agent 的 `configOptions` 直接 `extend`。不改 `last_used_template_id`，避免批量循环后最后一个模板成为用户默认入口。

同时修正现有默认模板初始化：用用户默认选择填充新 slot；不要每次读取模板时重新应用默认值。读取、内置定义升级与模型应用各有独立入口。

## 7. 数据归属与生命周期

| 数据 | 权威来源／生命周期 | 是否持久化 |
| --- | --- | --- |
| 安装策略／Catalog | 构建产物，随应用版本变化 | 打包只读 |
| 实际 Agent 配置及可执行路径 | `SettingsConfig.agents`，新候选诊断成功后自动登记 | 是，不等待最终应用 |
| 默认执行偏好 | `SettingsConfig.defaultExecutionProfile` | 是，一份 |
| 首次引导生命周期（pending／deferred／completed／existing-install）、Git 跳过意图 | `SettingsConfig.onboarding` | 是 |
| 环境检查结果 | 现有后端环境请求持有 request ID、取消标志及完成报告；页面仅投影 | 否；退出释放、重进检测 |
| Agent 能力／诊断 | 既有 Provider diagnostic 领域 | 复用现有缓存，不复制到 onboarding |
| 当前模型选择／展开状态 | 向导页面局部草稿 | 否；最终提交才持久化 |
| 模板绑定 | `WorkflowTemplateStore` 内 `WorkflowModelBindings` | 是，继续使用既有 revision |
| 应用中的跨文件操作 | 一个有界的提交恢复记录 | 临时持久化，完成即清理 |

建议 onboarding 持久态为 `pending { gitDecision } | deferred { gitDecision } | completed { schemaVersion, completedAt, lastApplyId, requestFingerprint } | existing-install`。当前步骤由策略、意图和本次探测结果推导，不另存三个 `stepCompleted` 布尔值。`deferred` 只表示用户主动选择稍后完成首次配置，不表示配置已完成。

`gitDecision` 仅记录 `undecided | skip`；通过版本检查属于实时事实。`deferred` 保留进入该状态时的 Git 决策，后续从设置应用默认配置时可直接复用，不要求用户重复选择。只有运行环境报告已通过后才允许写入 `deferred`，因此该状态不绕过 npm 门槛；它也不保留 Agent、模型或工作流绑定的部分提交。已有用户主动进入引导时保留既有 completed/existing-install 状态，只有最后提交成功才更新默认偏好和工作流绑定；此前成功诊断的新 Agent 已独立保存，取消不删除这些成果，也不破坏既有配置。

新增持久数据的必要性：默认选择和首次完成意图无法从已有各模板绑定中唯一推导；跨文件操作 ID 表示一次明确应用，不能由单模板 binding revision 表达。所有模板、Agent、项目继续复用现有 canonical identity，不新增 workspace identity、候选 Agent 数据库或额外模型配置表。

内部发现使用本轮后端生成的 locator 定位尚未保存的候选；第三步选择器及模型选择只接收正式 `ManagedAgentId`。选择器是 Agent 管理事实源中健康项的过滤投影，不保存另一份候选配置，也不展示失败 Agent 或修复入口。异步结果携带探测 generation 与配置指纹，旧 Agent／旧模型的返回不得覆盖新选择；`unavailable { agentId, label, iconKey, reason }` 携带结构化原因 `not-installed | runtime-required { tool } | doctor-failed { launch: npx | local, error }`，后端不生成对客文案；Agent 管理页修改或删除配置时发布 `withdrawn { agentId }`，只清理本轮局部状态。

页面用一个局部 reducer 统一环境报告与检测阶段：环境请求区分 idle／checking／ready／failed，Agent 批次区分 idle／detecting／completed／failed。这里 ready 表示报告已完成即可继续，报告内容只用于展示与提示。reducer 同时维护可用项与暂不可用项，两者按 Agent ID 互斥：`ready` 移入可用项，非 `not-installed` 的 `unavailable` 移入暂不可用项，`withdrawn` 同时移除。返回步骤或刷新 Agent 不改变环境报告；用户主动环境重测会取消旧 Agent 批次并撤销预览。环境完成结果只在 request ID 仍匹配且未取消时保存；环境阻断或检测失败后，本批次不再接收后续结果。页面序号与既有 generation 拒绝迟到响应，不增加持久字段或第二份诊断事实源。

## 8. 接口设计

已落地的应用服务位于 `src/onboarding/` 与 `src-tauri/src/onboarding.rs`：

| 接口 | 输入 | 返回／边界 |
| --- | --- | --- |
| `get_onboarding_status` | 无 | 状态、策略摘要、渠道；不运行 Doctor、不读历史或模板正文 |
| `check_setup_environment` / `cancel_setup_environment` | request ID | 返回并保留 4 项检查的完整报告；同页主动重测替换旧请求与报告，退出引导释放；原生 `git`／`node` 每项最多 5 秒，`npm`／`npx` launcher 每项最多 15 秒 |
| `skip_setup_git` | 当前引导意图 | 更新 skip 意图并返回权威状态；没有 npm skip 接口 |
| `defer_setup` | 当前引导生命周期与本次环境报告 | 仅在 controller 持有已通过的环境报告时把 `pending { gitDecision }` 转为 `deferred { gitDecision }`；通过 `spawn_blocking` 执行配置写入与 runtime 启动，不阻塞 IPC/主线程；不修改 Agent、默认偏好或工作流；重复调用幂等 |
| `start_setup_agent_detection` / `cancel_setup_detection` | after cursor、refresh、事件 channel／generation | 校验本次后端环境报告，不启动环境命令；编排只读发现、有界诊断和成功后自动配置；不创建或迁移模板与默认配置；逐项返回正式健康 Agent 与进度 |
| `discovery::candidates`（内部） | 已配置实例、after cursor | 当前发布 11 个、第一阶段 13 个、第二阶段最多 14 个经过平台过滤的 Catalog 候选，以及每批至多 50 个自定义实例；当前产品验收规模不超过 50 个自定义 Agent，after cursor 只是有界防御边界；不隐式下载或登录 |
| `probe_setup_model` | 已配置且诊断成功的 Agent ID、model ID、generation | 复用模型探测，返回目标模型配置目录 |
| `preview_default_setup` | 默认选择、当前诊断引用 | 模板与 slot 更新数量、冲突、预览指纹；无写副作用 |
| `apply_default_setup` | 默认选择、预览指纹、operation ID | 读取本次后端已完成环境报告，校验能力与预览后提交；不启动环境命令，成功后前端重新读取状态。相同请求重投保持幂等 |
| `save_setup_preferences` | language、appearance | 只更新语言／主题并合并最新设置 |

不得让前端提交完整 Settings 或 WorkflowTemplateStore 替换后端事实。模型、预览与应用接口只接受正式 Agent ID，后端从已持久化实例解析配置并校验匹配的成功诊断，不信任前端传入的 `ready=true`、版本号、模板路径或 `isBuiltIn=true`。未登记的候选不能直接用于完成引导。

错误采用结构化 enum，IPC 返回 `{code, params}`，前端将其映射为本地化 `msg` 与动作；后端不返回对客中文／英文句子。建议错误覆盖：

```text
setup.runtime-missing / setup.runtime-version-unsupported / setup.runtime-version-unverified
setup.runtime-incompatible（单项检查 code）/ setup.probe-timeout
setup.agent-auth-required / setup.agent-protocol-unsupported
setup.model-config-invalid / setup.preview-stale
setup.configuration-conflict / setup.commit-failed
setup.recovery-required
```

参数包含工具、Agent ID、最低／实际版本、失败阶段或冲突模板 ID。原始故障日志有界、脱敏并按需展开，不展示完整 env、token 或密码。

Agent 检测的 channel 只发布 Agent 进度与结果，删除旧 `environment-checked` 事件；`failed { generation, code, params }` 保留完整结构化参数。本次环境未检测、检测中或已取消时返回 `setup.environment-check-required`；已完成的报告无论结果如何都不再作为命令级错误返回。最终应用使用相同后端校验契约。单项不兼容原因采用枚举序列化的 reason（probe-unavailable、node-mismatch、node-version-mismatch、npm-metadata-missing、npm-engines-incompatible、npm-version-mismatch、npm-package-mismatch），携带对应版本、路径或 engines 值，由前端七语言文案解释；启动器路径与真实进程路径不同本身不构成不兼容。

## 9. 原子提交、重试与恢复

现有 `write_json` 能保证单文件原子替换，不能保证 `settings.json` 与 `context/workflows.json` 同时成功。推荐在现有存储层增加**仅覆盖这两份配置的一次应用事务**，不迁移整个项目到新数据库，也不建立后台任务队列。

提交顺序：

1. 读取本次已完成的后端环境报告，校验所选 Agent 能力并准备数据；不启动环境命令，不能持锁等待外部进程或登录。
2. 获取统一配置写入短锁，重读并比对相关默认偏好、所选 Agent 配置、模板 revision、诊断配置指纹和策略版本；不一致返回 `preview-stale`。其他 Agent 新近自动登记不使预览失效，应合并保留最新 settings 中的全部实例。
3. 原子写入用户配置根的 `.onboarding-commit.json`，含 operation ID 和两目标文件精确 before/after 内容；`.configuration.lock` 复用进程内锁并叠加 fs2 OS 文件锁。
4. 原子写入完整 `workflows.json`；随后原子写入更新默认偏好和 completed 标志的 `settings.json`，保留此前所有自动配置的 Agent。此步骤不承担首次登记 Agent。
5. 验证两个结果均匹配目标后发布成功事件并清理 pending；清理失败时可报告已提交但待清理，不重复应用。

恢复规则：下次启动在暴露可运行状态之前读取 pending。两个文件均为 before 时可继续，before/after 混合时补齐剩余写入，均为 after 时只清理；任一文件既非 before 也非 after 时返回冲突并停止自动覆盖。恢复不重新联网、不重新启动 Agent，也不替换用户之后写入的数据。

所有会写这两个配置的入口（包括诊断成功后的自动登记）必须参与同一锁与 pending 检查；期间读取应看到一致快照或“应用中”。自动登记在最终提交短事务期间等待，之后合并最新配置，不能把正在诊断的 Agent 生命周期纳入该事务。现有 `with_file_lock` 是进程内锁，不提供跨进程保证；由于 CLI 和桌面共享设置，应复用可用的 OS 文件锁实现，缺少时使用标准／成熟库补齐同一用户配置根的短时进程间锁。手工编辑造成的冲突由指纹发现，不能宣称文件锁能够约束外部编辑器。

同 operation ID 重试相同请求返回相同结果，不增加绑定 revision；相同 ID 配不同内容返回冲突。已完成标志携带最近 apply ID，足够覆盖首次提交断线重试，不引入无限增长的幂等表。

共享设置读取携带临时文件 revision，保存以 CAS 校验并在同一锁内返回本次提交的 revision。调用方需要继续编辑或条件回滚时必须使用该提交 revision，不能复用写入前的版本。既有 IM 凭据安装的设置保存与失败回滚也进入此边界，回滚不得覆盖后续修改。

工作流模板存储遵循同一 revision 契约：初始化、迁移、修复及普通编辑只要实际写入 `workflows.json`，都必须在配置锁内读取提交后的 `FileRevision` 并回填到返回的 `WorkflowTemplateStore`。不得让内存对象继续携带写入前 revision，否则紧接着的合法保存会被 CAS 误判为并发冲突。

pending 最多一条，位于现有用户配置作用域；内容可能包含设置中的私密配置，沿用严格文件访问权限，不写入日志、遥测或普通诊断导出，完成即删除。读写失败保持未完成／恢复中，不能以部分成功进入主界面。

## 10. UI、国际化与资源生命周期

- 保留三个步骤，第三步末尾的应用预览不是第四个页面；标题、操作区固定，中间单滚动区。
- 使用现有 shadcn/ui 控件和主题 token；窄窗口纵向排列，长路径安全断行，支持键盘、焦点与读屏。
- 选择器只展示可用 Agent；已安装但不可用的 Agent 进入暂不可用折叠区，只展示原因与“查看运行环境”，不提供单项重试或程序选择。选择器在收到 `unavailable` 或 `withdrawn` 后移除对应项并禁用提交，避免保留失效选择。
- 检测结束（无论是否有可用 Agent）或检测失败后提供“稍后设置”次按钮和一句结果说明；仅在运行环境检查已完成且 Agent 检测已结束时显示；Node/npm/npx 缺失只限制对应启动方式，不阻断稍后设置。检测中或环境检查失败时不显示。按钮使用现有 Button ghost 变体，窄窗口与长译文随底栏换行。
- 任何预计超过 300ms 的操作在触发后 100ms 内显示局部中间态；刷新保留成功项和当前选择。
- 环境检测通过既有 Tauri Channel 投影逐项进度；Git 的完成结果可先放行对应步骤，Node/npm/npx 的初步版本结果不作为联合就绪事实。最终报告到达后丢弃迟到进度，重测开始即替换旧报告。
- 检测状态、诊断状态和草稿留在向导局部，不把每次变化加入 App 全局高频 Context。
- 安装后点击重新检测时刷新实际 PATH；无需用户反复重启客户端。既有 Agent 仍无法解析时在 Agent 管理页配置已安装路径并重新诊断，引导不承载程序选择或失败修复 UI。
- 七种界面语言同步增加 key，以 zh-CN 为语义源；不得在 Rust 中拼接对客安装说明。提示词正文无需改动；如实际实现新增提示词，另行遵守 prompt-authoring 规则。
- 离开向导取消探测与监听，回收临时 ACP 会话、进程树和临时目录。切换模型时合并同 Agent 请求，旧 generation 不能回写。
- 仅进入向导时暂停其重复的后台自动诊断入口，或接入现有 single-flight；不能同时启动独立向导扫描与全量周期 Doctor。

## 11. 性能评审与验收目标

### 11.1 数据规模

| 数据 | 规模／增长 | 控制方式 |
| --- | --- | --- |
| 环境检查 | Git、Node、npm、npx 共 4 项 | 每项有限时命令，结果逐项返回 |
| 精选 Agent | 当前 14 项 | 先按当前平台过滤，再读取打包摘要；仅对本机已准备的候选启动有界自动诊断 |
| 自定义 Agent | 当前产品验收规模不超过 50 项 | 内部候选每批至多 50 项并保持有界诊断；超过 50 项的跨批场景不是本版本产品承诺，UI 仅投影已验证且已配置项 |
| 内置工作流 | 标准版 2 个，WB 3 个，后续受构建清单约束 | 在第三步预览时读取；算法 O(内置执行 slot 总数) |
| 用户模板存储 | 与用户模板数及正文大小一起增长 | 现有单 JSON 预览／提交为 O(文件大小)，不假装只扫描内置就免去解析成本 |
| 模型／配置目录 | 依 Agent 返回，可能变化 | 初始目录来自自动 Doctor；额外模型详情只探测选中 Agent／模型，复用现有有界诊断快照 |
| Task／Run／历史／项目文件 | 无界增长 | 不在本次引导读取或遍历 |

### 11.2 性能预算与测量范围

- 引导空壳与 loading 在触发后 100ms 内可见；状态 IPC 不等待外部进程，参考验收机 p95 ≤200ms。
- 本地版本命令按工具类别使用有界截止时间：原生 `git`／`node` 5 秒，经 Windows shim 冷启动的 `npm`／`npx` 15 秒；结果立即投影，异常慢命令不会阻塞其它项。两类预算必须来自同一环境策略，不得在探测实现里按名称特判。
- Doctor 沿用既有 3 分钟总预算和 4 个全局并发上限，不强行缩短为几秒；单个慢 Agent 不影响可见列表和其它可用选择。
- 全局发现进程数量有界，不按 Catalog 总数启动远程 npm 下载；11/13/14 项都只探测本机已准备候选，首屏检测网络请求为 0。
- 以 1,000 个用户模板、约 10 MiB 模板存储作为初始压力样本，预览／提交在后台执行，记录解析、校验、写入、锁持有时间；参考机目标分别 ≤1 秒。若超标，先拆分纯读取／维护路径或内置模板投影，不先加缓存。
- 生产构建测量干净环境、已安装环境、单项超时和上述大模板样本；保存基线与结果，不用开发模式主观观感验收。

2026-10-21 Windows release 存储样本：1,000 个用户模板、14,419,075 bytes，预览 146ms、应用 514ms，均通过各 1 秒预算。测试使用注入的环境健康结果，外部进程、UI p95、独立解析／锁持有时间及跨平台数据尚未作为此结果验收；详见开发计划中的命令和范围说明。

2026-11-03 将模板初始化与升级从 Agent 检测入口移到无副作用的内存预览后，Windows release 复测 1,000 个用户模板、14,419,075 bytes：预览 205ms、应用 426ms，均通过各 1 秒预算。Debug 构建同一样本预览 796ms，说明只读预览已避开整份模板的持久化差异序列化；应用仍只在最终确认时执行完整迁移、校验与两文件提交。测试继续使用注入的环境健康结果，不覆盖外部进程、UI p95、独立锁持有时间及跨平台数据。

2026-11-04 超时预算复核：统一 5 秒预算把“工具不可用”和“Windows `npm.cmd`／`npx.cmd` 冷启动较慢”混为一类，属于探测策略的数据模型缺陷。本机实测 `npm.cmd --version` 约 6.8 秒，真实探针因此稳定把可用工具判为超时；改为按工具类别拆分 policy 预算后，`npm`／`npx` 恢复 ready。该调整不新增自动重试、缓存或后台轮询，最坏失败等待仅在 `npm`／`npx` 单项从 5 秒增至 15 秒，四项仍并行且逐项渐进展示，正常路径耗时不变。

评审结论：主要风险来自外部进程冷启动、npx 按需下载和已有单文件模板存储，不是三步 UI 本身。自动诊断只覆盖已准备的本地候选，沿用有限并发且成功一项展示一项；每个新 Agent 至多一次配置写入，重复成功只更新诊断，不重复注册。保持首屏隔离与短提交边界，不引入长驻预热进程、无界缓存或额外轮询。

## 12. 验收与回归测试

所有实现先从领域接口固定验收，再做桌面实际流程验证。使用可注入命令执行器、假 ACP server 和临时用户目录，不依赖开发者电脑上的真实安装状态。

| 场景 | 必须固定的结果 |
| --- | --- |
| 干净首次启动，Git 未装 | 可显式跳过；界面不伪装 Git ready |
| Git 版本边界／Windows vendor 后缀／异常输出 | 复用现有解析，最低版本上下界行为一致 |
| Node 缺失／过低／不可执行 | 警告展示原因；允许 Agent 检测，本机 Agent 可正常发现和提交 |
| npx 缺失、启动失败或超时 | 提示 npx 启动的 Agent 暂不可用；这类候选不启动进程，返回 `runtime-required` 并在暂不可用区展示 |
| npm/npx 版本不受支持、engines 不兼容或实际 Node 不一致，但 npx 能启动 | 警告展示原因与实际 Node；npx 候选照常 Doctor，失败时附带版本或环境提示 |
| 离线但本地环境齐全 | 环境步骤成功，不请求下载站 |
| PATH 安装后变化、多套 Node、路径包含空格或中文、Node 启动器与真实 `node.exe` 路径不同 | 重测和正式执行使用同一有效程序组合；Node/npm/npx 观察到同一真实运行时即通过，启动器路径差异本身不阻断 |
| Node.js 23.11.0 等稳定但未验证版本 | 显示“未验证版本”警告但允许继续；npx 候选诊断失败时提示当前版本与推荐版本 |
| 第二步已通过，进入第三步／返回／刷新 Agent／最终应用 | 始终复用本次报告，不追加环境命令或环境等待阶段 |
| 环境报告被释放后重测／取消，旧事件迟到 | 重测完成后继续 Agent；取消后可再重测；旧批次不覆盖当前状态 |
| 检测结束只有已安装但不可用的 Agent | 显示“检测到的 Agent 暂不可用”，折叠区默认展开并按原因展示；未安装项不展示 |
| 普通检测异常／正常结束无 Agent | 前者提示检测失败，后者才显示正常空结果 |
| 默认配置直达入口，环境检测缓慢或有异常 | 初始检测仅一次；完成前不启动 Agent；完成后无论结果都继续 Agent 检测，运行环境步骤显示警告 |
| 未检测／主动重测中／旧请求取消后绕过页面调用 | 后端拒绝 Agent 检测和最终应用；迟到的旧成功结果不能恢复已失效报告 |
| 配置预览后本次后端报告取消或主动重测失败 | IPC 保留结构化原因；撤销预览、禁用提交，恢复后保留模型／思考强度／权限草稿并要求重新预览；应用不重跑命令 |
| 全新空配置，本机已安装 ACP 命令 | 不依赖管理页预先添加，使用本机命令和 ACP 参数诊断，成功后自动登记 |
| 用户在 Agent 管理页为 Gemini／WorkBuddy／Qwen Code 显式选择本机程序后重新诊断 | 只覆盖程序位置，使用对应 `localLaunch.args`；不得携带 npx 的 `-y` 或包名参数；引导下次运行只复用匹配的健康快照 |
| 全新空配置，仅有完整 npm 缓存且无全局命令 | 不依赖私有 `_npx` 字段，校验真实依赖与安装锁后直接诊断；成功项自动登记 |
| 缓存安装锁缺失、包名或精确版本不一致 | 不得直接启动该缓存，沿用已有受控准备或不可用路径 |
| 已安装 Claude／Codex／Pi CLI，尚缺 ACP 适配组件 | 自动进入对应适配器的受控准备与诊断，成功并保存后才算可用 |
| 只有 npx 启动器，没有对应 Agent 或已准备包 | 不下载全部精选 Agent，不判为 Gold Band 可用 |
| 当前 OS／CPU 被 Catalog 排除 | 内置候选不进入引导；直接创建由后端返回 `agent.unsupported-platform`；既有实例和引用不被删除 |
| Qoder 本机 CLI 与 Registry 固定 npx fallback | 两条路径分别使用各自命令与参数并独立通过 Doctor；不得用本机较新版本替代固定 fallback 的证据 |
| ZCode 尚未通过真实后端发布门槛 | 不进入发布 Catalog 和引导候选；社区桥接器 metadata 或 mock 握手不能解除阻断 |
| 管理页已有成功诊断且配置 fingerprint 未变 | 引导直接复用健康快照，不重复启动 Doctor，不改写管理页诊断 |
| 管理页诊断缺失或失败 | 引导隐藏该 Agent，不展示失败详情或修复入口；用户在管理页重新诊断后再运行引导 |
| 管理页上次诊断成功，但配置 fingerprint 缺失或已变 | 管理页显示“需重新诊断”；引导执行一次受控重诊断，成功后展示，失败不覆盖已有快照 |
| 既有 Agent 未登录／权限请求／取消 | 分别呈现状态；不自动批准，不遗留进程 |
| A Agent 诊断很慢，B 通过 | B 可继续选择；不等待全量结果 |
| 待诊断、诊断中、需登录、失败与成功项混合 | 第三步只展示成功且已持久化项，其余不作为禁用选项出现 |
| 多个新 Agent 诊断成功，只选一个为默认 | 全部成功项自动出现在 Agent 管理；仅选中项成为最终默认 |
| 自动配置成功后返回／退出引导 | 已配置 Agent 保留；默认偏好与工作流尚未应用 |
| 同 ID 重复发现／重复成功／已有用户配置 | 仅一个正式实例，不覆盖既有配置，不触发保存后重复 Doctor |
| 诊断通过但配置或诊断落盘失败 | 不发布可选项；重试或重启后安全恢复，无需用户重复添加 |
| 诊断期间用户修改／删除／取消 | 旧结果不覆盖、不复活被删除项 |
| 已选 Agent 后续诊断失败 | 从可选列表移除，保留草稿引用并禁用提交；管理页保留异常配置 |
| 预览后其他 Agent 自动登记成功 | 最终提交保留新增项，不因无关登记报预览失效 |
| 快速切换 Agent／模型，旧结果迟到 | 不覆盖新选择；单 Agent 诊断不并发 |
| 模型无目录／目录查询失败／切换后的参数不兼容 | 区分 provider-default、错误与冲突，不默默丢参数 |
| Agent 返回通用参数／模型专属参数，或旧默认配置含参数覆盖 | 模型、思考强度、权限复用工作流控件及能力映射；预览／提交只包含当前模型可配置项；切换模型恢复对应思考草稿，不暗中转提交未展示的通用配置 |
| 应用默认模型 | 全部内置 slot 更新，包括可选入口；结构、Profile、权限语义符合规则 |
| 用户模板、已有任务、定时任务、运行快照 | 字节或语义保持不变，不能批量污染 |
| 预览后模板或 Agent 配置被编辑 | 提交返回 stale／conflict，不覆盖新修改 |
| 每个持久化边界注入失败并重启 | 恢复到完整旧态或完整新态；不显示虚假完成 |
| 模板首次初始化／迁移／修复写入后立即继续编辑并保存 | 返回对象的 revision 与磁盘已提交内容一致，不产生虚假的配置冲突；再次写入后 revision 继续更新 |
| 同请求重复提交、并发提交、不同请求复用 ID | 幂等／拒绝冲突，绑定 revision 不重复递增 |
| 新版新增内置 slot | 只初始化新 slot，旧模型选择不回退为 Claude |
| 中途退出／正常升级／保留配置重装／手动重跑 | 分别恢复未完成、维持完成、维持完成、显式应用 |
| 长路径、七语言、深浅主题、窄窗后拉宽 | 布局、焦点、可读性与操作反馈符合项目规则 |
| pt-BR 375×667 引导展示可用 Agent | 选择器、状态和底部操作在内容宽度内，无页面横向滚动；拉宽后保持正常层级 |

真实 UI 验证必须启动前端并优先 deep link `#/onboarding`；依项目规定先尝试内置浏览器，再按可用性选择后备工具。原生安装、PATH 刷新和隐藏进程需额外在 EXE 验证。结束关闭本次测试资源。

## 13. 过度设计自评审与交付边界

**必要新增**：首次启动协调服务、持久完成意图、默认执行偏好、非 PATH 程序定位，以及两文件应用的恢复记录。它们分别对应当前无法表达的用户意图、安装位置与提交一致性。

**明确复用**：Agent ID／实例、ACP client、Doctor 限流与超时、模型目录、execution slot、revision、原子写与 UI 组件。不新增通用向导框架、包管理器、下载解压服务、Agent 协议桥、长期队列或模型推荐系统。

本轮交互调整复用现有 Agent 配置和诊断事实，只补齐“成功 → 保存 → 发布可选项”的转换，不新增自动注册持久状态机；所有诊断成功项自动配置与默认模型最终应用分别提交，避免把用户未选择的成功 Agent 丢弃。

2026-10-22 检测频率复核：此前每次进入第三步及最终应用均重新探测，增加等待并可能被偶发超时推翻刚通过结果，属于检测生命周期设计过度防御。改为现有环境请求在本次引导内保留一份权威报告，后续只校验该报告；无新增依赖、持久字段、TTL 或轮询。报告固定 4 项、读取与展示 O(4)，正常引导由三轮环境命令减为一轮；只有主动重测增加一轮，不并发启动额外扫描，不扩大全局订阅或读取历史。

**本期范围**：检测、官方安装指引、连接／认证入口、模型选择、预览与批量应用、异常恢复和回归。官方供应商登录／迁移仍由供应商持有；“安装完成”与“账号可用”单独验证。

**发布前必须关闭的技术风险**：现有全部精选 npm 包与 Node/npm 的兼容矩阵；无 workspace 时既有 Agent 的诊断与认证边界；现有模板读取混入维护写入的边界；所有共享设置写入者是否加入事务锁。以上是研发验证项，不要求用户自行调研。

2026-10-21 已实现主流程，完成接口、前端和 IAB 验证；具体命令、数量与性能结果维护在配套开发计划的实施记录。IAB 使用隔离 DEV fixture，不能替代真实 EXE、供应商账号或跨平台发布矩阵验收，环境策略仍保持 candidate。

2026-10-26 补充 Windows 真实 EXE 与现有受控 ACP fixture 验收：首次环境展示、主动重测、pending 状态关闭恢复、两个成功 Agent 自动保存并出现在 Agent 管理、标准版 2 个内置模板共 10 个 slot（7＋3）的应用与业务界面导航通过。fixture 使用 provider-default，此结果不覆盖真实供应商认证、完整模型／思考／权限目录、安装后 PATH 或 WB；具体隔离方式、自动更新干扰与临时资源清理限制见配套计划第 8 节。本轮未改变设计或产品实现，无新增抽象与运行性能影响。

### 13.1 2026-11-04 PR #20 复核修正

本次三个问题分别属于现有正确设计的实现缺口：模板写入未把新 revision 带回内存对象；显式本机程序只替换了 command、未同步切换到 `localLaunch.args`；修复列表虽然允许外层换行，操作组仍被 `shrink-0` 固定。修复统一收敛到既有 revision 写入边界、Catalog 启动契约和 shadcn/Tailwind 响应式布局，不增加 onboarding 状态、实例字段、依赖、缓存或专用断点逻辑。

过度设计复核：revision 在现有保存函数内统一回填，避免只补初始化调用点；本机程序参数继续复用 Catalog 静态元数据；UI 只调整现有 flex 与 Button utilities，新增复杂度与三个已复现不变量相匹配。

性能复核：模板仅在实际写入后、持有既有短配置锁时增加一次定点文件 revision 计算，成本与刚写入文件大小线性一致且不进入启动纯读取路径；Agent 解析仍为固定 Catalog 查询与 PATH 定向查找；UI 仍是固定数量元素的 CSS 布局，无新增 IPC、扫描、订阅、缓存或 JavaScript 测量。

## 13.2 2026-11-05 引导诊断事实源与失败项展示修复

- 现象：引导页面展示“检测未通过的 Agent”及重试／选择程序操作；已在 Agent 管理页诊断正常的 Codex、CodeBuddy，在引导再次探测失败后被管理页显示为失败，手动重新诊断才恢复，重复运行引导会再次覆盖。
- 根因：引导把一次性探测失败写入 `desktop/agent-diagnostics.json`，与 Agent 管理领域的 canonical Doctor 快照形成第二个写入事实源；已配置 Agent 也被引导重复启动独立 Doctor，迟到或环境差异会反向污染管理页状态。
- 修复：已配置 Agent 按稳定 ID 与当前配置 fingerprint 查询 canonical 诊断；仅 `available=true` 且 fingerprint 匹配时直接投影为引导可选项。诊断缺失、失败或过期的已配置 Agent 从引导隐藏，失败不写回管理页。未配置的内置候选仍执行只读发现和受控 ACP Doctor，只有成功才登记配置并保存健康快照。
- 删除失败项展示和 `retry_setup_agent`／本机程序选择链路；channel 使用无对客文案的 `unavailable` 事件清理局部选择状态，前端不渲染失败列表、错误详情或修复按钮。Agent 管理页仍是重新诊断和修复既有配置的唯一入口。
- 红测试证据：前端新增不可用 Agent 不显示名称、失败区块或修复操作的回归；领域新增匹配健康快照复用、失败／缺失／fingerprint 过期隐藏及未配置候选进入探测的接口回归。旧实现前端测试稳定显示失败列表，后端诊断覆盖路径会把健康快照改为 `available=false`。
- 自动化验收：`npx vitest run --config web/vitest.config.ts web/tests/onboarding.test.tsx` 37 项通过；`cargo test -p gold-band --lib setup_reuses_only_matching_healthy_configured_agent_diagnostics` 通过；`cargo check -p gold-band-desktop` 通过，保留既有未使用代码告警。

过度设计复核：复用现有 Agent ID、配置 fingerprint、Doctor 快照、generation 和配置提交锁；删除引导重复诊断与失败写入，不新增状态机、持久字段、缓存、队列或依赖。

性能复核：当前 11 个内置候选和不超过 50 个自定义 Agent 的规模下，已配置健康项只做一次内存 Map 查询并减少外部进程；只有未配置候选启动既有有界 Doctor，并发上限、截止时间、I/O 范围和 UI 状态订阅均不扩大。

### 13.3 2026-11-09 首次安装 Agent 检测后的受控退出修复

- 现象：首次安装用户在 Agent 步骤检测结束仍无可用 Agent 时只能重新检测或关闭客户端，无法进入主界面；同时产品要求检测结束后允许用户先进入应用，即使已有可用 Agent。
- 根因：`OnboardingGate` 把 `pending` 作为硬门禁，第三步的 `canSelect` 又要求已选择可用 Agent；持久态没有“用户主动稍后设置”的合法状态。属于正确设计下的实现缺口，不是探测逻辑错误。
- 修复：新增 `deferred` 生命周期态与 `defer_setup` 命令；命令复用 `SetupController.checked_environment()`，只在本次运行环境报告已通过时把 `pending` 转为 `deferred`，不修改 Agent、默认偏好或工作流，成功后启动业务 runtime。Agent 步骤在检测结束（无论是否有可用 Agent）或检测失败时显示“稍后设置”，点击后持久化并进入主界面；下次启动看到 `deferred` 直接挂载业务 App，设置页的重新运行引导保持可用。
- 审查修复：`deferred` 保留 `gitDecision`，`defaults::apply` 对 `pending` 与 `deferred` 统一消费该决策；`defer_setup` 改为 `async` + `spawn_blocking`，避免配置写入和 runtime 启动阻塞 IPC/主线程。
- 红测试证据：`enters the business application when no Agent is available and the user chooses to set up later` 在旧实现上失败于找不到“稍后设置”按钮；放宽为“检测完成即有可用 Agent 也显示”后，`offers set up later after detection completes even when an Agent is available` 在修改前同样失败于按钮不存在。两次修改后对应测试均通过。另补 `deferred_setup_survives_restart_and_is_idempotent`、pending 批次结束清理和 deferred 启动挂载业务 App 的回归。
- 后端回归：新增 `deferred_setup_preserves_git_skip_for_later_apply`，固定跳过 Git 后 defer 仍保留决策，并可在 `request.git_skipped=false` 时完成默认配置应用。
- 自动化验收：`npx vitest run --config web/vitest.config.ts web/tests/onboarding.test.tsx` 43 项通过；相关 i18n、设置与关闭保护测试 24 项通过；`cargo test -p gold-band --lib onboarding::initialization` 6 项通过；`cargo test -p gold-band --lib onboarding::tests::deferred_setup_preserves_git_skip_for_later_apply` 1 项通过；`cargo check -p gold-band-desktop --all-targets` 通过，保留既有未使用代码告警；`npm run web:build` 通过。浏览器 fixture 以 `onboardingFixture=no-agents` 验证无 Agent 空结果、稍后设置、URL 离开引导页及 600×640 窄窗布局，页面 error 日志为空；浏览器无法承载依赖 Tauri 的业务运行时，业务主界面挂载由 DOM 回归测试固定。
- 过度设计复核：只新增一个带 Git 决策的生命周期状态、一个命令和一个次要按钮；复用既有环境报告、`SettingsConfig` 事实源和 `start_business_runtime`，不新增 aggregate、持久表、缓存、队列或依赖。
- 性能复核：defer 路径只执行一次已通过报告的读取、一次 settings 原子写和一次既有 runtime 启动调用，并通过 `spawn_blocking` 将阻塞性 I/O 移出 IPC/主线程；不增加扫描、轮询、高频 IPC 或 UI 全量订阅，数据规模与引导原有 11 个精选候选／50 个自定义实例一致。

## 14. 官方资料与代码证据

保留资料的核实日期为 2026-10-16，本次范围调整未重新核验外部版本；以下 URL 是引用来源，不是产品内硬编码下载地址。

| 编号 | 来源 | 地址 |
| --- | --- | --- |
| S1 | 已读取：Node.js 官方发布索引，包含 LTS 标记 | `https://nodejs.org/dist/index.json` |
| S2 | npm 官方 package 元数据 | `https://registry.npmjs.org/npm/10.9.0`；`https://registry.npmjs.org/npm/11.19.1` |
| S3 | Node.js 22.20.0 发布说明 | `https://nodejs.org/en/blog/release/v22.20.0` |
| S4 | ACP 认证协议 | `https://agentclientprotocol.com/protocol/v1/authentication` |
| S5 | ACP 会话配置协议 | `https://agentclientprotocol.com/protocol/v1/session-config-options` |

代码证据定位：`src/git.rs`、`src/process.rs`、`src/config/mod.rs`、`src/config/managed_agents.rs`、`src/agent_catalog.rs`、`src/acp/client.rs`、`src/workflow_model_binding.rs`、`src/app/mod.rs`、`src/storage/mod.rs`、`src-tauri/src/commands.rs`、`web/src/App.tsx`、`web/src/lib/workflow-model-bindings.ts`、`scripts/prepare-agent-catalog.mjs`。

## 13.3 2026-11-12 陈旧诊断投影与引导复用一致性修复

- 现象：CodeBuddy 在 Agent 管理页显示诊断正常，但首次引导不展示；用户无法从两处状态判断该 Agent 是否真的当前可用。
- 根因：管理页只按 `diagnostic.available` 投影“正常”，引导还要求诊断保存的配置 fingerprint 与当前配置一致。配置或 schema 变化后，两者对同一快照给出相反结论；引导把曾成功但已陈旧的诊断当作不可用，静态隐藏了本可自动恢复的 Agent。
- 修复：诊断投影新增 `stale`，仅在 `available=true` 且 fingerprint 匹配时显示“正常”，曾成功但 fingerprint 不匹配时显示“需重新诊断”并从可用 Agent 集合排除。引导遇到该状态时复用既有受控 Doctor 重测一次；失败只产生本轮 `unavailable`，不写回或清空管理页快照。
- 测试：接口测试固定 stale 诊断返回 `Probe`，Desktop 投影测试固定 stale 状态与 `available=false`，前端 DOM 测试固定“需重新诊断”文案；所有语言目录同步维护该状态。
- 过度设计与性能复核：复用现有 fingerprint、Doctor 调度、单 Agent 互斥和失败不落盘边界，不新增持久字段、状态机、缓存或队列。每个已配置 Agent 仅增加一次内存 fingerprint 计算；只有陈旧且上次成功的 Agent 进入既有有界 Doctor，正常健康项仍不启动进程。

### 2026-11-17 分支迁入边界

环境报告的检查完成态统一作为发现与稍后设置的前提；Agent 是否需要 npx 继续按其实际启动路径判断。`defer_setup` 复用同一份检查报告，不要求 npm 可用，不自动选中 Agent，不改变默认绑定；成功后进入主应用。保留 deferred 状态的 Git skip 决策，设置页重跑引导仍可完成配置。
