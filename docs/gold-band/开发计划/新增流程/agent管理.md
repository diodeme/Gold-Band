现在应用程序的侧边栏是任务编排、知识库、模型管理
- 2026-09-20：AUTO / AI-DYNAMIC runtime continue 只传 snapshot `configOptionOverrides`，不得把冻结作者态 `config_options` 再 merge 回来；新节点按已观测 `modelBoundCatalogs[modelId]` 静默 retain。Direct 运行中追问走 prompt queue，不会再 apply 作者态 leftover。
- 2026-09-20：工作流编辑器保存校验按所选模型的 `modelBoundCatalogs` 投影判定 `thought_level` / `model_config`，与 Inspector 菜单同一份目录；不得用 Doctor 当前表把 Grok 的 effort / Fast 报成不属于当前 Agent。
- 2026-09-19：配置展示名带 option id，例如「思考强度（effort）」；名称与 id 仅大小写不同时不重复。菜单与回滚分割线共用标签。
- 2026-09-19：复合菜单一条 `thought_level` 标「思考强度」；多条时 `thinking`/`effort` 中文为「深度思考」「思考强度」，`context` 为「上下文」。英文保持协议名。
- 2026-09-19：正式会话活目录回写作者态当前表：更新诊断 `configOptions` 的 `model.currentValue` 与绑定行，保留模型/权限 options 列表和 `checked_at`。未观测模型复用最后一次观测（Doctor 或会话），已观测模型仍只画自己的 `modelBoundCatalogs`。
- 2026-09-19：发起会话不再用 Doctor 当前表拦截 `thought_level` / `model_config`；不支持的项由 `session/new` 回滚为不指定。若仍出现非绑定类 `option-unsupported`，显示「当前模型不支持这项配置」，不再落到「操作失败，请重试」。
- 2026-09-19：会话与作者态统一按 `modelBoundCatalogs[modelId]` 缓存。已观测只画自己的表；未观测复用当前配置发起，返回后更新该模型缓存。会话栏读本会话 snapshot 的 map，不得把省略表时的上一模型目录写成当前模型。分割线为「当前Agent暂不支持配置：{{names}}。系统已将其回滚为不指定。可停止对话后修改。」
- 2026-09-18：作者态按 `modelBoundCatalogs[modelId]` 缓存每个模型的 `thought_level` / `model_config`；Doctor 与正式会话活目录都可写入。发起会话时 `session/new` 活目录若已不含作者态 option id，或值不在可选列表中，按切模型同一规则 remap 思考强度或回滚为不指定；分割线写出「当前Agent暂不支持配置：{{names}}。系统已将其回滚为不指定。可停止对话后修改。」。已观测模型不得把另一模型的 `model_config` 画进所选模型。
- 2026-09-18：Select / 列表式 Agent 选择器的触发器和选项都展示 registry icon 与 display name；Select 身份槽按 SVG viewBox 原样绘制，会话栏继续保留 Codex 等视觉重量缩放。选择器禁用原因与 Agent 管理横幅使用同一条 compact raw 首行，不再把 `session-request-failed` 显示成没有具体原因的「请重试」。
- 2026-09-18：Select / 列表式 Agent 选择器的触发器和选项都展示 registry icon 与 display name，与 Direct 药丸、侧栏和画布共用 `AgentIdentityLabel`；不得只显示名称或改用通用 Bot 图标。
- 2026-09-18：切到 Luna 等 option id 不同的模型时，思考强度按档位 value remap，不得把 High 当成不支持清空；复合菜单固定思考强度在 Context/Fast 之前。Doctor 默认模型的依赖项不得覆盖另一模型的会话目录。
- 2026-09-18：作者态切模型时保留当前思考强度 / Fast；发起会话后若新模型目录不支持，回滚为不指定并在该节点/会话 timeline 写入分割线 `systemNotice`。Doctor 默认模型的依赖项不得覆盖另一模型的会话目录。
- 2026-09-18：Composer 按官方 `category=model_config` 把 Fast 纳入与思考强度同一复合下拉；Doctor 默认模型的依赖项不得覆盖另一模型的会话目录。
- 2026-09-15：Doctor 与正式 ACP 连接共用全局 `initialize` 客户端能力，新增 `_meta.parameterizedModelPicker`。依赖该声明才展开思考强度 / 模型参数的 Agent，诊断目录与运行期 `configOptions` 必须一致；不按 Agent ID 分叉 handshake。
你现在先新增个agent管理吧
agent管理主要是负责管理支持接入的ACP agent
当前改为维护构建期精选 ACP Agent Catalog，固定提供 `claude-acp`、`codex-acp`、`cursor`、`gemini`、`codebuddy-code`、`goose`、`qwen-code`、`opencode`、`kimi`、`amp-acp`、`pi-acp` 十一类模板，并支持用户自定义 ACP Agent；GLM 不进入本轮范围
agent管理页面主要就是agent卡片和新增agent按钮
agent卡片支持删除、修改、环境诊断操作（检查agent环境是否正常，提供手动检测能力，后台每1分钟自动检测一次agent环境），并显示agent的诊断状态（最好用对应图标）；doctor 失败时在状态旁显示问号帮助入口，该入口使用 shadcn/ui `Popover`，点击后展示 raw 原因（ACP JSON-RPC `message`、有界故障 stderr、启动失败 `osError`）与配置帮助，没有原始原因时才回退本地化错误句；提示参考 ACP Registry 配置命令、参数、环境、网络和认证状态，ACP Registry 链接到 `https://agentclientprotocol.com/get-started/registry`，使用产品链接样式，点击后通过 `openWebTarget` 在内置浏览器打开。Agent 管理页复用当前工作空间的 draft 右侧工作区投影，未打开工作区时右栏保持收起。卡片内容需要有稳定左右内边距；最近检测时间展示为本地系统时区 `YYYY-MM-DD HH:MM:SS`；手动诊断运行中显示圆形加载动效，完成后根据结果显示数秒成功或异常横幅，异常横幅只展示「环境诊断未通过：{{reason}}」，`reason` 为 raw 原因首行，完整原因放到问号入口；成功态横幅与成功状态图标需复用主题 success token，避免页面硬编码颜色；诊断命令 `npx -y @agentclientprotocol/claude-agent-acp@latest` 用于启动 Claude ACP adapter，首次运行可能通过 npm 下载依赖而耗时 1 分钟以上；每个 Agent 每轮诊断的初始化、会话创建、命令发现、清理和周期重试共享 3 分钟预算，结束、失败、超时或客户端关闭都必须退出诊断进程树，不能阻塞客户端
补充诊断环境要求：
- ACP adapter 与 doctor 必须复用 `process` 模块的跨平台 PATH 解析接口，并以首次出现项为准去重。Windows 优先级固定为“Agent 显式配置 PATH → 当前桌面进程 PATH → 用户注册表 PATH → 系统注册表 PATH → 平台通用目录”；每次创建 ACP 进程前直接通过注册表 API 读取 `HKCU\Environment\Path` 和 `HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment\Path`，展开 `%VAR%` 并按大小写不敏感去重，不调用 `reg.exe`。macOS / Linux 优先级固定为“Agent 显式配置 PATH → 用户登录 Shell PATH → 当前桌面进程 PATH → 平台通用目录”；首次使用时从 Unix 账户信息选择默认 Shell，以 `-ilc` 登录交互模式读取环境，设置 2 秒总超时并回收超时进程，成功或失败结果按应用生命周期缓存。Shell profile 噪声通过输出边界隔离，只读取 `PATH`；失败时回退当前进程 PATH。Unix 按大小写敏感语义合并，并补全 `~/.nvm/versions/node/*/bin`、`~/.local/bin`、`~/.cargo/bin`、`~/.volta/bin`、`/opt/homebrew/bin`、`/usr/local/bin` 等通用位置；所有平台都禁止维护 Kimi、Cursor、OpenCode、Scoop、npm 等 Agent 或安装器目录特判。该解析仅位于 doctor / ACP 进程创建边界，不进入会话消息热路径
- Windows 裸命令 PATH 查找统一限定为 `.exe`、`.com`、`.cmd`、`.bat` 候选，优先原生可执行文件并允许 npm `.cmd` wrapper；忽略 `.ps1` 和无扩展名 Unix shim，避免把 `#!/bin/sh` 文件交给 Win32 `CreateProcess` 后产生 OS error 193。显式带扩展名命令保持原名；PowerShell 脚本必须由用户显式配置 `pwsh` / `powershell -NoProfile -File`
- “使用本地 Claude”只影响 Claude ACP adapter 的 `CLAUDE_CODE_EXECUTABLE` 注入。Windows 上需要同时兼容原生安装和 npm 安装：原生安装优先使用 PATH 中的 `claude.exe`；npm 安装若 PATH 目录暴露 `claude.cmd` shim，则读取 `.cmd` wrapper 内容并解析其实际指向的 native `.exe`，例如 `%dp0%\node_modules\@anthropic-ai\claude-code\bin\claude.exe`，不能把 extensionless `claude` shell shim 传给 adapter，也不能依赖固定 npm prefix 拼接路径；若找不到原生 binary，则不注入环境变量。macOS / Linux 继续按 PATH 查找可执行 `claude`；Unix npm shim 本身是可执行脚本，不需要像 Windows 一样解析 `.cmd` wrapper。
- 本地 Claude 注入调用点必须复用统一解析函数，并通过单元测试覆盖 Windows npm `.cmd` 内容反查场景，避免后续合并时只保留 helper/测试却把 adapter 启动调用点回退成仅查 `claude.exe` 或固定目录拼接。
- 项目级 `configs/app-config.toml` 提供 `requireLocalClaudeExecutable` 诊断开关，默认关闭。开启后，“使用本地 Claude”但未解析出 native executable 时直接让 doctor / 会话启动失败，不再进入 `claude-agent-acp` / Claude Agent SDK 内部 fallback，用于验证本地发现逻辑；临时排障也可用环境变量 `GOLD_BAND_REQUIRE_LOCAL_CLAUDE=1` 覆盖开启。
- 若 adapter 启动失败，doctor 结果必须保留底层 OS 错误文本，例如 `No such file or directory (os error 2)`，不能只显示泛化失败文案
新增 Agent 使用带搜索框的 shadcn/ui `Command + Popover` 选择器，支持从十一个内置模板或“自定义 Agent”进入同一编辑 Sheet。内置模板按构建期 Registry 快照预填命令和参数；npx 类 Agent 使用 Registry package，其他 Agent 默认调用 PATH 中用户已安装的可执行文件。Gold Band 不下载、解压或托管 Agent 二进制。已新增过的内置类型不可重复新增。Pi ACP 使用 Registry 生成的 `npx -y pi-acp@<version>`，用户仍需自行安装 Pi coding agent 并保证 `pi` 位于 PATH。

Catalog 与实例必须分域管理：
- 2026-09-08 启动配置归属修正：schema 11 不再保存内置实例的命令和参数，旧设置加载时原子回写移除，读取时投影当前 Catalog；覆盖历史内置启动定制，保留名称、图标、环境、目录和能力配置。保存接口忽略内置命令/参数输入，编辑器对应字段只读，自定义 Agent 继续全部可编辑。复用稳定 ID、现有设置序列化与 Catalog，不增加依赖、持久身份、缓存或队列；开销限于小型配置内存处理，无新增网络、全量历史加载或热路径 I/O。
- 2026-09-09 只读启动字段视觉修复：Catalog Agent 的命令与参数保留原生 `readOnly` 以支持聚焦、选择和复制，统一使用 `muted` 表面与弱化文字；可编辑的显示名称、环境变量和目录字段统一使用 `background` 表面，并覆盖 Textarea 的主题深色变体，移除只读字段的编辑态 focus ring；自定义 Agent 仍使用可编辑样式。DOM 回归测试固定只读、非 disabled、共享底色、弱化文字与可编辑字段边界。本改动仅调整静态 class 投影，不引入状态、依赖、I/O 或额外渲染。第一次修复前测试在 Catalog 灰态断言稳定失败，后续用户反馈暴露深色主题控件底色竞争；后续最小测试在只读分组和可编辑分组断言稳定失败，修复后 Agent 管理相关测试及 `npm run web:build` 通过。Chrome deep link 已在浅色、深色下验证两组字段颜色清晰，命令聚焦无编辑态高亮，页面无溢出或重叠；临时主题、页面和测试服务已恢复或清理。
- 验证记录：旧配置迁移最小测试已确认修改前失败。清理磁盘后重新编译验证通过：启动配置集成测试 2 项、配置模块单测 53 项、设置加载与持久化测试 2 项、桌面 Agent 保存接口测试 5 项；前端三组共 20 项测试、TypeScript 检查及 `npm run web:build` 生产前端构建通过，内置浏览器验证内置只读与自定义可编辑。构建仅有现有未使用代码、混合导入和包体积警告；本轮未制作安装包或运行 macOS 真机验证。
- `AgentCatalogEntry` 是构建期模板；`ManagedAgentConfig` 是用户实例
- 新建时复制模板用户字段；内置 Agent 的命令和参数始终由当前 Catalog 提供，既有实例随客户端升级更新启动配置，其余用户字段不受影响。自定义 Agent 全部配置由用户维护。
- 构建/发版前拉取 `https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json`，校验精选十一项齐全后生成 Registry 快照、Catalog JSON 和官方 SVG 图标并打包
- 提供显式离线脚本，允许基于已提交 Registry 快照重建 Catalog；常规发版刷新失败时必须失败退出，不发布残缺 Catalog
- 构建期版本覆盖由 `configs/agent-catalog-policy.json` 的 `versionPins` 统一管理，例如 `"claude-acp": "<exact-version>"`；无对应项时使用 Registry 版本。在线刷新与离线生成共用覆盖逻辑，同步覆盖 Catalog 版本和 npx 包参数，不修改原始 snapshot；已有内置实例在新客户端中使用覆盖后的启动配置。仅支持 npx Registry 包的精确版本，未知 ID、非法策略和不支持的分发类型必须失败。在线生成检查固定 npm 版本存在性；离线生成不联网检查。
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
- workflow 节点的权限模式只能从当前 agent doctor 返回的 `supportedModes` 中选择；切换 agent 时清空旧权限模式和 Auto Accept，不做跨 agent 权限模式映射
- Auto Accept 是 client overlay，不是 doctor `supportedModes` 的一项；所有 ACP Agent 的权限下拉都展示同一套开关
- workflow 画布不得维护内置 provider → icon 硬编码表，必须按 provider 从当前 managed Agent registry 读取实例 icon；这同时覆盖后续 Catalog Agent、自定义 Agent、用户上传 data URI 和空值默认 icon
- ACP 权限模式与节点 Profile 分层生效：权限模式使用 Agent 实际暴露的 mode API 控制工具授权，Profile 继续约束角色职责。实时切换权限成功后不得改写 Profile；例如 `pf-builtin-plan` 在 `yolo` 下仍然只负责规划。验收时必须同时检查 outbound mode 请求、Agent 响应中的 current mode 与节点 Profile，不能仅根据模型是否愿意改代码判断权限是否生效

## 本轮实现与验收记录（2026-08-07）

### 2026-09-18 Agent 卡片按列表容器宽度降列

- 根因：卡片网格使用整窗 `md/xl` breakpoint。打开内置浏览器后中间栏变窄，窗口仍是 `xl`，继续强制三列，命令/参数被压成逐字竖排，标题被截成 “Agent …”。属于正确的 1/2/3 列设计没有按容器实现，不是要改工作区折叠阈值。
- 方案：复用角色列表的 `@container/agent-list` 与 `@2xl` / `@6xl` 阈值；兼容档登记 `agent-list` measured fallback。卡片内部摘要固定两列。integrated Header 用最小标题宽度换行，不再按整窗 `sm` 强行同行。
- 过度设计评审：不新增列数 state、不改 `centerMinWidth`、不另做卡片布局状态机。
- 性能评审：完整档只有 CSS container query；兼容档多一个已有 measured observer，只发布离散档位。已配置 Agent 数量为小型列表，无额外请求或全量扫描。
- 验收：修复前布局契约测试因缺少 `@container/agent-list` 失败，Header 测试因仍使用 `sm:flex-row` 失败。修复后相关 8 个文件 41 项通过。1920 宽窗下，右栏收起时三列；把内置浏览器拖到约 940px 后中间栏 394px 降为单列，标题完整显示「Agent 管理」，命令/参数保持两列摘要，诊断/修改/删除同一行。

### 2026-09-18 Agent 诊断横幅与 raw 原因分层

- 根因：ACP JSON-RPC 失败（如 CodeBuddy `Authentication required`）被收成 `acp.session-request-failed`，`RuntimeErrorInfo.raw` 没有进入诊断 snapshot；横幅又用本地化主句替换了原始原因，问号 Tooltip 同样看不到 ACP `message`。
- 方案：诊断 snapshot 增加可选 `raw`，doctor 原样保留 ACP 错误对象。Agent 管理异常横幅只显示「环境诊断未通过：{{reason}}」，`reason` 为 ACP `message` / 故障 stderr / `osError` 的首行；异常旁问号 Popover 展示完整原文。工作流、会话和运行模式选择器与横幅使用同一条 compact 首行，不展开完整 stderr。
- 过度设计评审：复用既有 `DiagnosticError` 与页面 Popover，不新增错误码分类、登录流或第二套诊断状态。
- 性能评审：`raw` 仅为单次 JSON-RPC 错误对象；Popover 内容在打开后才进入 DOM；不进入会话热路径，无额外轮询或缓存。
- 验收：`doctor_diagnostic_error_preserves_session_request_raw` 修复前因缺少 `raw` 字段编译失败；前端 copy 测试固定 Authentication required 进入 raw 投影；横幅测试确认不内嵌本地化句和 stderr；问号点击后才出现 raw 与 ACP Registry 链接。

### 2026-09-18 Doctor 对客错误保留有界故障 stderr

- 根因：诊断失败把内部 `Display`（`ACP adapter transport interrupted`）写入 snapshot `reason`；已分类的 adapter 故障 stderr 只进 DEBUG，对客看不到 `ENOENT` / `npm error`。
- 方案：诊断 snapshot 改为结构化 `error { code, params }`；doctor 将 transport interrupt 映射为 `acp.adapter-exited`，启动失败映射为 `acp.adapter-start-failed` 并保留 OS 文本。连接层缓存有界故障 stderr（2000 字符），doctor 失败时写入 `params.reason`。Agent 管理横幅/帮助 Tooltip 展示本地化主句加原始输出；选择器只展示主句。不改聊天 transport 文案、默认 INFO 策略，也不为 npx 特判。
- 过度设计评审：复用既有 stderr 分类、RuntimeErrorInfo 和 Git `params.reason` 模式；不新增状态机、队列或持久 identity。
- 性能评审：故障缓冲有界，仅 doctor 失败路径额外等待最多 250ms 排空 stderr reader；不进入会话热路径，不把全文写入 INFO。
- 验收：`doctor_initialize_exit_keeps_classified_failure_stderr`、transport/start-failed 映射与 stderr 有界拼接测试通过；桌面 `background_doctor` 4 项通过；前端诊断 copy/横幅及相关 Agent 测试 37 项通过；`tsc` 通过。Doctor `session/new` 超时回归通过。

### 2026-09-09 Doctor 超时与跨 Agent 隔离

- 根因：早期 Doctor 只做有超时的 initialize，增加 session/new 以发现模型与权限能力后未扩展超时；周期诊断改为并行、目录改为 Agent 隔离后，仍持有整个批次的全局运行锁。一个存活但不响应的 adapter 因而可以阻塞后续其他 Agent 的诊断。
- 修复前证据：模拟 adapter 正常 initialize 后不回答 session/new，500ms 测试预算未生效，旧实现直到 2 秒后 fixture rescue 才返回，断言失败；另一个测试以 channel 保持首个诊断运行，第二个诊断无法取得执行资格，独立执行断言失败。两项均已实际观察到失败。
- 实现：单 Agent 每轮共享 180 秒截止时间，周期重试沿用剩余预算；所有诊断与命令目录入口按 Agent ID 互斥，总 adapter/批次 worker 上限均为 4。周期结果逐项发布，手动 IPC 进入 blocking 执行器，保留配置版本校验、保存解耦、请求合并和失败日志回收。
- 过度设计评审：复用标准库 Mutex/Condvar、既有 ACP 请求轮询、RuntimeErrorInfo 和进程组管理；运行集合仅表达正在占用的 Agent ID，最多 4 项，无新增依赖、持久字段、并行业务身份或通用调度框架。
- 性能评审：内置 Catalog 为 11 项，并允许自定义 Agent，批次输入规模为已配置 Agent 数 N。遍历 O(N)，批次线程与活动 adapter 均至多 4，互斥集合至多 4 项；无新增历史读取或持续轮询，结果事件仅在单项完成时发送。配置/结果短锁不覆盖 provider 等待。验收检查一个阻塞 Agent 下其他项可完成、相同 Agent 不重叠、并发上限以及完成后的资源释放。
- 验收完成：原 session/new 与跨 Agent 阻塞复现测试转绿；ACP client 138 项通过（另有 1 个仅由父测试启动的 ignored adapter fixture），桌面 state 相关 38 项通过，前端 Agent 诊断/启动字段/workflow 健康相关 6 项通过。覆盖 initialize、session/new、session/delete、session/close 超时及阶段错误码，超时后 adapter 的 TCP listener 已关闭、PID 文件移除，成功后能力保留且临时目录删除。
- 接口与规模验收：Codex probe 由 channel 保持运行时，CodeBuddy probe 可完成，最终两者结果均正确落盘；同 Agent 等待不占额外 adapter 名额，释放后可继续运行；1,000 个模拟 Agent 全部处理且 worker 数不超过 4。重试使用完全相同的截止时间，已耗尽预算不重试；保存提交不等待运行中 doctor、连续保存请求合并等既有测试继续通过。
- 前端验收：DOM 测试确认等待时按钮 loading/disabled、重复点击不重发，失败结果到达后停止旋转并恢复重试，其他 Agent 的按钮不受影响。`tsc -p web/tsconfig.build.json --noEmit` 和 Vite 生产构建通过；保留既有大 chunk、混合静态/动态 import 和 Rust dead-code 警告。内置 iab 不可用，按项目规则使用已连接 Chrome deep link `/chat/agents`，确认页面渲染及诊断完成后的按钮恢复；浏览器使用前端 mock，实际 ACP 超时与进程回收由 Rust 子进程测试验证，未重新连接用户真实 Codex/CodeBuddy 账号。测试页面与本次 Vite 进程在验收后关闭。

### 2026-10-08 Claude / Codex ACP 升级

- 将已有精确 pin 更新为 Claude ACP `0.87.0`、Codex ACP `2.1.1`，基于当前 Registry snapshot 重建 Catalog，不覆盖其他 Agent 的配置变更。
- 根因是协议入口仍假定旧 adapter 输出：补齐 ACP v1 压缩能力协商、稳定 ID、生命周期、摘要 patch、回放及新轮入口；工具增量更新保留省略字段，累计 Codex 命令输出；远程提问识别 Claude 无 AIR 的自定义答案字段。
- 复用现有 canonical timeline、用量状态、工具详情和组件，不新增独立状态库、依赖或可选功能。过度设计评审：仅迁移现有体验；性能评审：无全量历史加载，身份查找沿用索引，命令输出有上限，详情仍按需获取。
- 验收包含失败用例复现、Rust 单测与 mock adapter、前端 DOM 测试和浏览器页面验收；真实 provider 的压缩触发、取消与恢复、工具输出及提问交由用户运行后确认。
- 已验收：新增 7 项协议/入口/回放/stdio mock 回归通过，远程干预 19 项通过，前端 124 项通过，Catalog 7 项通过；TypeScript 检查和 Vite 生产构建通过。ACP 扩展回归 599 项通过、3 项忽略，另有 `path_files_are_listed_in_the_prompt_text_even_with_optional_capabilities` 因未修改的 `runtime/user_files.md` 使用 CRLF 而断言 LF 失败，单独记录，不计为通过。
- iab 不可用后使用已连接 Chrome，deep link 到 browser mock 会话，确认压缩完成/进行中/中断/未知状态及压缩前用量显示；DOM 回归另验证同一压缩实体跨更新不重挂、终态计时停止。页面 mock 与真实 provider 行为分开验收。
- 待用户真实验收：更新后的两个 Agent 各触发一次压缩；压缩/工具执行中停止后继续、重开会话；确认 Codex 持续输出命令结束后详情保留、文件 diff 正确；Claude 选择题和自定义答案可提交（使用 IM 时也验证远程选择）。

#### 新版文件变更、认证与停止 mock 补充验收

- 新增 10 项回归：Rust 文件变更 4 项、认证/停止 stdio 子进程 2 项、认证错误码分类 1 项，前端错误提示 1 项及终态按钮 DOM 2 项。
- 文件样例按 Codex ACP `2.1.1` 的普通 ACP 输出构造，覆盖同一工具同一文件的多个 hunk、Unicode 与无末尾换行、持久化后重读、新增/删除空文件及采集/展示大小限制。纯重命名的空 diff、上游省略的大型 diff 不生成虚构内容；这验证现有边界，不承诺恢复上游未提供的全文或重命名关系。
- 认证 mock 先复现 `-32000` 被统一包装成 `provider.acp-prompt-failed`，使既有认证分类失效。修复为使用 schema 的 `ErrorCode::AuthRequired` 映射到 `provider.auth-required`；保留原始错误、手动恢复和无自动重试。中文错误消息单测确保分类不依赖英文文案；普通失败 fixture 改用协议规定的 InternalError `-32603`。
- 子进程验收覆盖认证失败状态落盘与发布、手动第二轮成功、旧错误清除；停止验收覆盖发送一次 `session/cancel`、未完成工具结束、轮次转 cancelled/idle、后续轮次成功。DOM 验收覆盖停止中禁用、终态覆盖迟到的本地 busy 标记、发送按钮与输入恢复、第二次发送后重新进入忙碌态。
- 本轮结果：Rust 升级相关 13 项及错误/失败回归 4 项全部通过；1 个 ignored 测试仅作为上述父测试启动的 mock adapter 子进程入口。前端 composer、错误提示、状态投影与停止反馈共 102 项全部通过。未运行真实账号会话或新增页面验收；仍需用户验证真实认证恢复及真实工具执行中停止后继续。
- 完成评审：复用既有 schema、错误类型、会话生命周期、组件与测试工具，无新依赖、持久字段或产品入口；生产代码仅增加常数时间错误码分类，无新增 I/O、扫描、缓存或渲染工作。mock 子进程在测试结束或断言失败时关闭，临时工作目录自动回收。

#### 压缩摘要右侧阅读（2026-10-08）

- 视觉调整：“查看摘要”移至用量所在的第二行，复用 Button link 变体，普通字重、次要文字色和轻量箭头；悬停加深并显示下划线，窄窗口可换行。仅调整既有组件的展示层级，无新增状态、依赖、数据读取或渲染订阅。
- 视觉调整验收：摘要相关 5 项 DOM 回归、TypeScript 检查和生产构建通过；独立浏览器确认浅/深色主题下为 12px 次要文字与透明背景，hover 下划线及右侧 Tab 打开正常，窄窗口无横向溢出。继续复用既有布局，不增加运行时开销。

- 完成且包含非空文本摘要的压缩行新增“查看摘要”，按完整会话 locator 和压缩事件 ID 打开/复用右侧 Tab；默认渲染 Markdown，复用现有只读 Markdown/源码组件，支持源码切换与复制。
- 列表只投影摘要可用性；正文按需读取 canonical timeline 单项及摘要 blob，避免长摘要被通用 raw 截断。新增异步 IPC、加载/失败重试/清空提示和七语言文案，Tab 状态不保存正文，不增加依赖或独立摘要存储。
- 过度设计与性能评审：复用现有身份、索引、工作区与 Markdown 组件；仅用户打开时读取选中事件，冷索引加载沿用有界索引机制，无首屏逐项详情请求或 raw 日志全量读取。长文本成本限于激活摘要的读取和渲染；不做假设性缓存或全局订阅。
- 验收：2 项 Rust 接口测试覆盖长摘要 blob 完整读取、文本顺序、空摘要及跨 attempt 隔离；新增 5 项前端回归覆盖有无摘要、重复打开、locator 状态、加载、渲染/源码、错误重试、清空和迟到响应。相关前端共 14 项通过，TypeScript 与 Vite 生产构建通过。iab 不可用、Chrome 连接策略失败后，用独立 agent-browser 会话 deep link 验证右侧打开、源码切换、长文、窄窗口与重新拉宽。真实 provider 摘要需用户在更新后的桌面客户端触发 compact 后确认。
- 页面补充验收：浅色与深色主题的摘要正文、按钮和 hover 正常；重复点击仍为单个 Tab，页面无横向溢出。跨 attempt 展示前缀通过现有 `goldBandScope.originalId` 还原为 canonical 事件 ID，导航回归固定此契约。验收后关闭专用浏览器、前端进程并清理临时截图。

### 2026-09-30 固定压缩事件兼容版本

- 通过现有 `versionPins` 固定 Claude ACP `0.81.2`、Codex ACP `1.13.1`；这些正式版仍输出 `_meta.contextCompaction`，供现有压缩生命周期识别使用。
- 基于现有 Registry snapshot 离线重建 Catalog，版本元数据与 npx 启动参数同步固定；上游 snapshot 保留原值。
- 回归测试同时模拟两个 Agent 的 Registry 版本变化，验证本地固定版本仍优先。后续接入 ACP 预览版 `compaction_update` 并完成验收后再评估解除固定。
- 过度设计与性能评审：复用现有配置和生成接口，无新增依赖、状态或运行时开销。

### 2026-09-25 解除 Claude / Codex 版本固定

- 删除 `claude-acp` `0.72.0` 与 `codex-acp` `1.12.0` 两项 pin，`versionPins` 为空对象；在线刷新后 Catalog 为 claude-agent-acp `0.81.2`、codex-acp `1.13.1`，其余模板随 Registry 小版本更新。原 `0.72.0` 用于规避 SDK `0.3.257` 的 macOS 12 启动回归，解除后需要在 macOS 12 回归验证。

### 2026-09-08 构建期本地版本覆盖

- 原设计以在线 Registry 提供模板，发布方缺少固定版本能力；补齐生成期策略覆盖，保留每次 build 在线刷新的行为。当前 Claude 固定 `0.72.0`，删除配置项即可解除固定，不使用 `-1`。
- 最小失败测试确认旧生成器忽略本地 pin，输出 `0.73.0` 而非 `0.72.0`；实现后 Catalog Node 测试 7 项通过，覆盖版本和启动参数一致性、原始快照不变、未配置项、scoped/unscoped 包、附加参数与环境保留、非法配置及上游版本变化。
- 使用现有 snapshot 离线重建本地 Catalog，Claude 元数据与包参数均为 `0.72.0`；npm 元数据确认该版本存在并依赖 SDK `0.3.252`。未执行应用编译或 macOS 真机验证。
- 过度设计与性能评审：复用现有 catalog 生成管道与成熟 npm 包规格解析库，仅增加小型 JSON 配置和构建期校验。每次生成对十一项线性处理，在线校验仅对固定项请求 npm 元数据，单次请求超时 30 秒；运行时只投影当前内嵌 Catalog，无额外联网。

- 已新增 Registry 快照准备脚本、离线重建脚本和 Node 单元测试，精选列表固定为十一项，包含 Amp、Pi 且排除 GLM
- 已将 Catalog 通过 Rust `include_str!` 和 Vite public assets 打包；内置实例的启动配置读取当前 Catalog，其余设置保留用户值。
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


## npx 安装缓存修复

- npm/npx Agent 诊断失败且当前诊断原始原因包含 npm ENOENT 与明确的 `_npx/<安装目录>/package.json` 缺失路径时，异常状态旁提供“修复缓存”。这是候选入口，实际可操作性由后端预览核实；不解析共享 npm 日志、不处理任意内部文件缺失。
- 点击后立即显示检查中状态，使用同一启动环境、工作目录及 npm 安装查询实际缓存根目录。支持常用 cache/userconfig/globalconfig 参数；无法确认配置或路径时拒绝修复。
- 后端只接受 Agent ID，预览返回已校验目录与诊断/配置/目录集合指纹；确认提交 ID 与指纹，重新核实后执行。支持本次错误中的多个明确目录，去重，上限 16 个；不清空整个 _npx，不清理 _cacache，不增加独立应用缓存。
- 确认弹窗说明共享影响、重新下载可能性，并允许展开查看目标目录。按钮为“清理并重新检测”；取消不修改磁盘。
- 只删除实际缓存 _npx 下的直接安装子目录。拒绝越界、父级跳转、安装目录软链接/junction，以及已恢复的 package.json。目录删除使用标准库且不跟随内部链接。
- 用户确认后直接尝试清理校验通过的目录，不要求退出 Agent，不主动关闭连接、杀进程或强制解锁。确认弹窗提示“正在使用这些缓存的 Agent 可能中断，建议先停止相关任务。”修复持有当前 Agent 的诊断互斥，等待已开始的诊断结束并暂缓后续诊断，结束后自动释放；修复操作本身禁止并发重复执行。占用或权限错误逐项返回，允许部分清理失败。
- 清理结果逐项返回；全部成功后重新诊断当前 Agent，明确区分清理失败、清理成功但诊断失败、诊断通过。操作期间禁用重复及当前卡片冲突操作。正常列表只做有界错误文本解析，无 npm 查询或缓存扫描；查询限时 10 秒，目录操作放后台线程。
- 实现复用 npm、现有进程组、shadcn/ui AlertDialog 与诊断接口。仅保留修复操作互斥及现有当前 Agent 诊断互斥，无持久缓存或任务队列。删除 I/O 与目标目录规模相关，不做全缓存扫描或预计算体积。

### npx 缓存修复验收（2026-09-29）

- 后端 4 项测试通过：多个安装目录定向清理与正常缓存保留、ENOENT/越界/已恢复文件拒绝、链接拒绝、同一 npm 安装和 Agent 环境解析；并覆盖重复修复拒绝；进程占用限制后续由下述回归修正。
- 前端相关 25 项测试通过，包含确认前不清理、即时 loading、取消、重复提交阻止、逐项失败和清理后诊断。TypeScript、前端生产构建、桌面 cargo check 通过；保留既有构建告警。
- iab 与已连接 Chrome 不可用后，使用独立 agent-browser 会话完成真实页面模拟接口验证：普通/窄窗口/重新拉宽、明暗主题、长路径和清理后诊断失败反馈。没有对用户真实缓存执行清理。测试资源已关闭。
- 完成评审：复用现有 UI、诊断和进程管理，无新增依赖或持久状态；正常列表无额外 I/O，目标清理在后台执行。外部程序占用仍可能导致部分清理失败，由逐项结果明确反馈。

### 修复入口被常驻进程阻塞的回归修正

- 根因：全局 npm/npx 进程计数把其他 Agent 的常驻连接当作清理前置条件。删除该计数及 Adapter/ManagedProcessGroup 接入，只保留修复操作互斥和当前 Agent 现有诊断互斥，不新增进程归属或关闭机制。
- 回归先在原实现观察到 running_npx_adapter_does_not_block_user_confirmed_repair 因 npx-cache.busy 失败；修改后同一测试通过，确认运行中的测试 Adapter 不阻止修复且不会被终止，重复修复仍被拒绝。后端缓存模块 5 项、前端修复交互 4 项测试通过，类型检查和生产构建通过；浏览器确认七语言中的简体中文风险提示呈现。
- 用户确认后可能中断共享缓存使用者；路径校验和逐项失败返回保持不变。修复期间等待当前已开始的诊断完成并暂缓同 Agent 后续诊断，不要求所有 Agent 退出。性能影响为减少启动链路锁与计数，无新扫描、缓存或持久状态。
