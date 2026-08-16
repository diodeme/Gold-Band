现在应用程序的侧边栏是任务编排、知识库、模型管理
你现在先新增个agent管理吧
agent管理主要是负责管理支持接入的ACP agent
当前改为维护构建期精选 ACP Agent Catalog，固定提供 `claude-acp`、`codex-acp`、`cursor`、`gemini`、`codebuddy-code`、`goose`、`qwen-code`、`opencode`、`kimi`、`amp-acp`、`pi-acp` 十一类模板，并支持用户自定义 ACP Agent；GLM 不进入本轮范围
agent管理页面主要就是agent卡片和新增agent按钮
agent卡片支持删除、修改、环境诊断操作（检查agent环境是否正常，提供手动检测能力，后台每1分钟自动检测一次agent环境），并显示agent的诊断状态（最好用对应图标）；doctor 失败时在状态旁显示问号帮助入口，该帮助入口统一使用随主题变化的浅色 shadcn/ui `Tooltip` 展示错误原因与配置帮助，悬浮或聚焦即可出现；提示参考 ACP Registry 配置命令、参数、环境、网络和认证状态，ACP Registry 链接到 `https://agentclientprotocol.com/get-started/registry`，点击后通过系统默认浏览器打开。卡片内容需要有稳定左右内边距；最近检测时间展示为本地系统时区 `YYYY-MM-DD HH:MM:SS`；手动诊断运行中显示圆形加载动效，完成后根据结果显示数秒成功或异常横幅；成功态横幅与成功状态图标需复用主题 success token，避免页面硬编码颜色；诊断命令 `npx -y @agentclientprotocol/claude-agent-acp@latest` 用于启动 Claude ACP adapter，首次运行可能通过 npm 下载依赖而耗时 1 分钟以上；诊断 initialize 最多等待 5 分钟，结束、失败、超时或客户端关闭都必须退出诊断进程树，不能阻塞客户端
补充诊断环境要求：
- ACP adapter 与 doctor 必须复用 `process` 模块的跨平台 PATH 解析接口，并以首次出现项为准去重。Windows 优先级固定为“Agent 显式配置 PATH → 当前桌面进程 PATH → 用户注册表 PATH → 系统注册表 PATH → 平台通用目录”；每次创建 ACP 进程前直接通过注册表 API 读取 `HKCU\Environment\Path` 和 `HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment\Path`，展开 `%VAR%` 并按大小写不敏感去重，不调用 `reg.exe`。macOS / Linux 优先级固定为“Agent 显式配置 PATH → 用户登录 Shell PATH → 当前桌面进程 PATH → 平台通用目录”；首次使用时从 Unix 账户信息选择默认 Shell，以 `-ilc` 登录交互模式读取环境，设置 2 秒总超时并回收超时进程，成功或失败结果按应用生命周期缓存。Shell profile 噪声通过输出边界隔离，只读取 `PATH`；失败时回退当前进程 PATH。Unix 按大小写敏感语义合并，并补全 `~/.nvm/versions/node/*/bin`、`~/.local/bin`、`~/.cargo/bin`、`~/.volta/bin`、`/opt/homebrew/bin`、`/usr/local/bin` 等通用位置；所有平台都禁止维护 Kimi、Cursor、OpenCode、Scoop、npm 等 Agent 或安装器目录特判。该解析仅位于 doctor / ACP 进程创建边界，不进入会话消息热路径
- Windows 裸命令 PATH 查找统一限定为 `.exe`、`.com`、`.cmd`、`.bat` 候选，优先原生可执行文件并允许 npm `.cmd` wrapper；忽略 `.ps1` 和无扩展名 Unix shim，避免把 `#!/bin/sh` 文件交给 Win32 `CreateProcess` 后产生 OS error 193。显式带扩展名命令保持原名；PowerShell 脚本必须由用户显式配置 `pwsh` / `powershell -NoProfile -File`
- “使用本地 Claude”只影响 Claude ACP adapter 的 `CLAUDE_CODE_EXECUTABLE` 注入。Windows 上需要同时兼容原生安装和 npm 安装：原生安装优先使用 PATH 中的 `claude.exe`；npm 安装若 PATH 目录暴露 `claude.cmd` shim，则读取 `.cmd` wrapper 内容并解析其实际指向的 native `.exe`，例如 `%dp0%\node_modules\@anthropic-ai\claude-code\bin\claude.exe`，不能把 extensionless `claude` shell shim 传给 adapter，也不能依赖固定 npm prefix 拼接路径；若找不到原生 binary，则不注入环境变量。macOS / Linux 继续按 PATH 查找可执行 `claude`；Unix npm shim 本身是可执行脚本，不需要像 Windows 一样解析 `.cmd` wrapper。
- 本地 Claude 注入调用点必须复用统一解析函数，并通过单元测试覆盖 Windows npm `.cmd` 内容反查场景，避免后续合并时只保留 helper/测试却把 adapter 启动调用点回退成仅查 `claude.exe` 或固定目录拼接。
- 项目级 `configs/app-config.toml` 提供 `requireLocalClaudeExecutable` 诊断开关，默认关闭。开启后，“使用本地 Claude”但未解析出 native executable 时直接让 doctor / 会话启动失败，不再进入 `claude-agent-acp` / Claude Agent SDK 内部 fallback，用于验证本地发现逻辑；临时排障也可用环境变量 `GOLD_BAND_REQUIRE_LOCAL_CLAUDE=1` 覆盖开启。
- 若 adapter 启动失败，doctor 结果必须保留底层 OS 错误文本，例如 `No such file or directory (os error 2)`，不能只显示泛化失败文案
新增 Agent 使用带搜索框的 shadcn/ui `Command + Popover` 选择器，支持从十一个内置模板或“自定义 Agent”进入同一编辑 Sheet。内置模板按构建期 Registry 快照预填命令和参数；npx 类 Agent 使用 Registry package，其他 Agent 默认调用 PATH 中用户已安装的可执行文件。Gold Band 不下载、解压或托管 Agent 二进制。已新增过的内置类型不可重复新增。Pi ACP 使用 Registry 生成的 `npx -y pi-acp@<version>`，用户仍需自行安装 Pi coding agent 并保证 `pi` 位于 PATH。

Catalog 与实例必须分域管理：
- `AgentCatalogEntry` 是构建期模板；`ManagedAgentConfig` 是用户实例
- 新建时将模板完整深拷贝到实例，之后 Catalog 更新不得修改任何既有实例
- 构建/发版前拉取 `https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json`，校验精选十一项齐全后生成 Registry 快照、Catalog JSON 和官方 SVG 图标并打包
- 提供显式离线脚本，允许基于已提交 Registry 快照重建 Catalog；常规发版刷新失败时必须失败退出，不发布残缺 Catalog
- Kimi 的主/兼容 Skills 目录为 `.kimi-code` / `.agents`；Amp 为 `.agents` / `.claude`；Pi 的全局主目录为 `.pi/agent`、项目主目录为 `.pi`、两端兼容目录均为 `.agents`

主 Skills 目录默认由全局与项目作用域共用。编辑 Sheet 在标题右侧提供带 Tooltip 的分裂图标按钮；开启后按钮呈选中态，单输入框拆为“全局主目录 / 项目主目录”，关闭后恢复共用主目录。数据层以可选项目主目录字段表达拆分状态，不维护独立布尔值。目录策略按作用域生成：全局写入全局主目录，项目写入项目主目录；两端读取时都在各自主目录后追加共用兼容目录，兼容目录始终只读。Catalog、设置实例、Tauri 输入/VM、SkillManager、原生命令扫描和同步状态必须消费同一目录策略接口，禁止在 Pi 调用点判断 Agent ID。

自定义 Agent 表单需要用户配置稳定 Agent ID、display name、icon、命令、参数、环境和主/兼容 Skills 目录。界面将原“Agent 类型”准确命名为“Agent ID”，说明其创建后不可修改。编辑器显式维护 Catalog 新建、自定义新建、已有实例编辑三种来源状态，Agent ID 锁定规则不得从当前输入文本推导；自定义输入即使暂时与 `kimi` 等 Catalog ID 相同，也必须允许继续输入后缀。Agent ID 的即时规范化必须识别 IME composition 生命周期，组合期间不改写临时文本，组合结束后再过滤为小写字母、数字和连字符。system prompt 能力由 Gold Band 内部维护，不提供用户开关；自定义 Agent 固定为不支持。icon 可使用系统文件选择器导入不超过 1 MB 的 PNG、JPEG、WebP 或 SVG，并以 data URI 持久化；新建自定义 Agent 与空 icon 默认使用项目 `gold-band` Logo，已有明确保存为 `agent` 的实例不迁移。主 Skills 目录允许为空，表示该 Agent 不参与 Skills 读写与同步。
agent需要有对应icon标识，参考 `docs\gold-band\资源\icon` 目录；应用打包实际读取 `web\public\agent-icons`，Cursor 图标也必须同步复制到该目录
新增agent时，已经新增过的agent类型，不能重复新增
agent配置需要做持久化管理；修改 Sheet 的参数和环境变量使用可换行的多行编辑区，编辑时不即时吞掉空行或换行；参数保存时按空格或换行拆分，环境变量保存时按行解析；保存成功后只清空当前 agent 的旧诊断状态，并由后端后台自动诊断该 agent 一次，保存接口不得等待本次或已经运行中的 doctor，持久化完成后前端立即关闭 Sheet 并提示“配置已保存，正在后台诊断”；自动诊断期间卡片显示诊断中并禁用重复修改、删除和诊断，完成后通过桌面事件刷新全局 Agent registry；新增、编辑或删除某个 agent 时，不允许把其他已诊断 agent 一并回退成未诊断

修改 Sheet 需要保存打开时的规范化配置快照，并与当前按真实持久化规则解析后的配置比较。无修改时禁用保存按钮，提交函数也必须二次拦截，不能调用 `update_agent` 或触发 doctor；参数空白布局变化、环境变量行顺序变化等规范化后等价的输入不算修改。

Agent `command` 在前端构建保存参数、脏状态比较以及后端 `ManagedAgentInput` 转配置时统一执行首尾 `trim`。仅修改命令前后空格不算配置变化，也不能触发保存和自动诊断；真实命令内容变化仍正常保存。

当前版本暂不对客开放跨端会话合并与外部会话同步配置：Agent 卡片、新增 Agent 和修改 Agent Sheet 均不展示两个选项。底层字段、持久化和 runtime 逻辑继续保留；编辑其他字段时必须保留已有隐藏配置，新建实例继续使用 Catalog/自定义 Agent 的既有默认值，后续具备自动能力发现与清晰使用场景后再开放。

Agent 实例新增两个独立能力配置：
- system prompt：默认关闭，仅 Claude 模板默认开启并使用 `_meta.systemPrompt.append`；能力不进入用户可编辑输入，自定义 Agent 固定关闭；关闭时新会话首个 user prompt 内嵌稳定上下文，恢复会话不重复注入
- 跨端会话合并：默认关闭；只有能力开启时才允许启用外部会话同步，保存边界必须把不合法组合归一化为关闭

删除所有 preset 白名单运行门禁。保存、doctor、Provider 构造和 workflow 校验只依赖合法 Agent ID、已持久化实例配置和实时诊断结果；自定义 ACP Agent 必须能走完整运行链路。原 `supported` 产品语义不再用于区分内置与自定义 Agent。

诊断生命周期补充：手动诊断、保存后自动诊断、每分钟周期诊断和命令目录刷新共享统一 doctor 运行边界。同一 Agent 保持 singleflight；不同 Agent 的周期诊断使用各自的 `doctor/acp/<agent-id>` 临时目录和 `provider.pid` 并行执行，禁止跨 Agent 清理诊断资源。配置保存使用独立短时提交锁，不能等待 doctor。诊断完成写入前再次比对配置版本，版本已变化则丢弃旧结果并继续处理最新版；同一 agent 的连续自动诊断请求按版本合并。手动诊断、保存后自动诊断和命令目录刷新不重试；每分钟周期诊断首次失败后重试一次，第二次仍失败才持久化异常与错误原因。工作流编辑器保留已配置但诊断失败的 Agent 及已有节点选择，将其置为不可用并继续阻止保存和运行。接口层单元测试需要固化目录隔离、周期诊断只重试一次、手动诊断不重试、最终错误落盘、保存提交不受运行中 doctor 阻塞、自动诊断请求去重与完成后可再次入队的行为。

补充实现约束：
- worker 节点中的 `provider` 字段显式声明 agent type，当前不提供默认 claude 兜底
- 当前 agent type 直接作为 registry key 使用，因此同一类型只能维护一份配置
- 节点详情页需要展示当前节点声明的 agent type，便于确认执行来源
- 工作流创建、修改和模板保存时，Agent 下拉只允许选择已配置且最近一次 doctor 成功的 agent；未诊断或诊断失败的 agent 不能进入 workflow
- workflow 节点的权限模式只能从当前 agent doctor 返回的 `supportedModes` 中选择；切换 agent 时清空旧权限模式，不做跨 agent 权限模式映射
- workflow 画布不得维护内置 provider → icon 硬编码表，必须按 provider 从当前 managed Agent registry 读取实例 icon；这同时覆盖后续 Catalog Agent、自定义 Agent、用户上传 data URI 和空值默认 icon
- ACP 权限模式与节点 Profile 分层生效：权限模式使用 Agent 实际暴露的 mode API 控制工具授权，Profile 继续约束角色职责。实时切换权限成功后不得改写 Profile；例如 `pf-builtin-plan` 在 `yolo` 下仍然只负责规划。验收时必须同时检查 outbound mode 请求、Agent 响应中的 current mode 与节点 Profile，不能仅根据模型是否愿意改代码判断权限是否生效

## 本轮实现与验收记录（2026-08-07）

- 已新增 Registry 快照准备脚本、离线重建脚本和 Node 单元测试，精选列表固定为十一项，包含 Amp、Pi 且排除 GLM
- 已将 Catalog 通过 Rust `include_str!` 和 Vite public assets 打包；创建时复制模板，已有实例不读取 Catalog 默认值
- 已开放自定义 Agent 创建、保存、doctor、Provider 和 workflow 运行链路，移除 preset 白名单门禁
- 已新增 Agent 搜索选择器、自定义入口、本地图标选择和可选 Skills 目录编辑项，并复用现有 shadcn/ui 组件；system prompt 与跨端会话能力当前均不向用户开放
- 图标编辑已收敛为预览、选择本地图片和恢复默认 Logo，不向用户暴露 icon key、URL 或 data URI 文本输入；既有图标引用保持兼容
- 图标命令按钮统一使用透明 ghost 样式，避免 outline 默认底色与相邻按钮 hover 底色叠加后被误认为“双选”；当前 hover 与键盘 focus-visible 反馈保持可用
- 图标区从单输入框 `<label>` 容器拆为 `fieldset + legend` 操作组，修复整行 label 将 hover/click 语义转发到“选择图片”的扩大命中范围问题
- 编辑器以单一生命周期状态统一管理 `open`、来源、Agent ID、表单、文本配置和初始快照；三个打开入口原子替换完整状态，关闭和保存成功只切换 `open=false`，退出动画期间保留当前 draft，下一次打开再整体替换，修复内容闪变造成的“抽屉关闭后又打开”错觉
- 公共 Sheet 已让 overlay 默认跟随 `modal`：桌面编辑/详情侧栏统一标记为非模态且不再渲染遮罩，窄屏工作区等真正模态的 Sheet 继续保留遮罩，消除保存/关闭时全局页面由暗变亮的闪烁
- Agent 删除确认框已将 `open` 与 target 统一管理；确认、取消或删除失败只关闭弹窗，退出动画期间保留 Agent 名称，避免文案中间消失
- 编辑器来源状态同时保存默认 icon key 与名称：Catalog Agent 的恢复目标为自身 Catalog 图标，自定义 Agent 的恢复目标为 Gold Band Logo；Rust 创建接口对空 icon 使用同一来源规则
- 内置单色 Agent icon 已通过统一图标 helper 适配深色主题；品牌彩色、自定义 URL 和默认 Gold Band Logo 不做反色处理
- 已将 system prompt 和跨端会话能力下沉到实例运行策略，补齐 schema v3 一次性迁移
- 接口级回归覆盖：精选十一项完整性、Amp/Pi/GLM 范围、全局/项目目录拆分策略、空 Skills 目录、默认 icon、跨端能力归一化、system prompt 注入策略和 Catalog 生成失败策略
- 验证结果：`cargo check --workspace --all-targets -j 1` 通过；Rust 库实际执行 518 项，517 项通过，本功能新增测试全部通过，唯一失败为既有 `acp::branches::tests::result_migration_v2_removes_legacy_background_acknowledgements`（期望 `queued`、实际 `completed`），单独复跑仍失败；桌面 `ManagedAgentInput` 命令边界测试 2 项全部通过
- 验证结果：Catalog Node 测试 2 项通过，Agent 管理及 workflow/run-mode 相关前端测试 42 项通过，`npm run web:build` 通过；全量前端 880 项中 875 项通过，剩余 5 项失败集中在本轮范围外的 right-workspace/file-link 既有改动
- 页面实测：通过 `/chat/agents` deep link 验证 11 个内置 Agent、Amp/Pi 搜索、自定义入口、目录拆分、默认 Bot icon、空 Skills 主目录保存、跨端能力联动和禁用态；页面 console 无 warning/error，测试页面、服务和临时进程已清理
- 2026-08-12：当前版本从全部 Agent 卡片、新增与修改入口隐藏“支持跨端会话合并 / 同步外部会话”；删除专用 Switch、Beta Badge 与帮助 Tooltip 的渲染代码，保留配置 DTO、持久化和 runtime 语义。接口回归固定卡片不展示高级字段，并验证编辑其他字段不会改写已有隐藏配置。
- 2026-08-08：新增 Agent 下拉收敛 cmdk active 态的视觉语义。`Command + Popover` 继续保留键盘 active 与回车确认能力，但下拉刚展开时不把第一项渲染为选中态；只有鼠标悬停项显示 hover 背景，避免“自定义 Agent”被误判为默认选中。前端测试固化该样式契约。
- 已将 macOS / Linux ACP 子进程 PATH 从“常见目录猜测”升级为通用登录 Shell 环境发现：从 Unix 账户默认 Shell 读取 `-ilc` 环境，使用 2 秒总超时、输出边界、进程回收和进程生命周期缓存；移除 `~/.opencode/bin` Agent 特判，并让通用可执行文件查找与 ACP adapter 共用同一解析入口
- PATH 接口验收：Windows `process` 测试 9 项、ACP adapter 测试 11 项全部通过，覆盖原生 binary、npm `.cmd`、无扩展名 Unix shim 与显式扩展名；Unix 专属 `process.rs` 最小工程在 `x86_64-unknown-linux-gnu` target 交叉编译通过；`cargo check --workspace --all-targets -j 1` 通过，仅保留本轮范围外的既有桌面端 warning
