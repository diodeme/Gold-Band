# Agent 管理页

## 1. 一句话定义
Agent 管理页负责维护当前桌面 workspace 可用的 Agent 配置，并提供诊断、编辑与删除能力。

---

## 2. 页面目标
当前桌面端需要把“节点声明用哪个 agent”和“这个 agent 实际怎么执行”分开：
- workflow 节点通过 `provider` 显式声明稳定的 Agent ID
- Agent 管理页负责维护这个 ID 对应的执行命令、参数、环境变量、Agent 目录和诊断状态

当前规则：
- Worker / Verify 节点必须显式声明 `provider`
- 当前不提供默认 Claude 兜底
- 当前同一 Agent ID 只能配置一份实例

---

## 3. 页面结构

```text
Page Header
- 标题
- 刷新
- 新增 Agent（下拉）

Agent Cards
- icon
- display name
- Agent ID
- command / args / env 摘要
- 诊断状态 / 最近检测时间（本地系统时区 `YYYY-MM-DD HH:MM:SS`）
- 诊断 / 修改 / 删除

布局要求：
- 页面级 Header 使用共享 `PageHeader` 的 `integrated` 管理页变体，与上下文管理、运行模式管理保持同一页面表面：标题左侧使用与侧栏一致的 `Bot` 语义图标，图标与标题组按视觉中心垂直对齐，并使用 `text-foreground` 随明暗主题自动反色；Header 采用 24px 水平内边距、32px 顶部内边距，并与主体保留约 28px 的纵向节奏；标题使用紧凑层级，不设置独立背景、模糊、投影或底部分割线；操作区在宽窗口与标题同行，窄窗口允许自然换行
- agent card 内容与卡片边缘保持稳定左右内边距，不允许内容贴边
- 列表默认优先采用高密度多列布局；桌面常见宽度下应尽量一行展示 3 张卡片，再逐级退化到 2 列或 1 列
- 不减少命令、参数、环境变量、最近检测等关键信息项，但需通过更紧凑的标题区、信息区和操作区压缩卡片高度
- 编辑 Sheet 头部、表单区和底部操作区需要保持统一左右内边距
```

---

## 4. 新增 Agent
新增按钮使用带搜索框的 shadcn/ui `Command + Popover` 选择器，列表来自构建期打包的精选 ACP Agent Catalog：
- Claude：`claude-acp`
- Codex：`codex-acp`
- Cursor：`cursor`
- Gemini：`gemini`
- CodeBuddy：`codebuddy-code`
- Goose：`goose`
- Qwen Code：`qwen-code`
- OpenCode：`opencode`
- Kimi Code：`kimi`
- Amp：`amp-acp`
- Pi：`pi-acp`
- 自定义 Agent：由用户输入稳定 Agent ID 和完整启动配置

限制：
- 已配置过的 Agent ID 不可重复新增
- Catalog 模板同时提供稳定 ID、名称、版本、图标、推荐命令、参数、主 Agent 目录、兼容 Agent 目录和已确认能力；新增时深拷贝为独立 `ManagedAgentConfig`，运行时不再根据 Agent ID 或 Catalog 推导实例配置
- Catalog 更新只影响之后新建的 Agent；已经创建的实例无论是否被用户编辑，都不跟随 Catalog 的版本、命令、目录或能力变化
- 新增时预填 ACP Registry 当前快照中的推荐命令、参数和 display name，用户可按本机安装路径调整；npx 类 Agent 使用 Registry package，Cursor、Goose、OpenCode、Kimi 和 Amp 默认调用 PATH 中已安装的可执行文件。Pi 使用 Registry 的 `npx -y pi-acp@<version>` 适配器，同时要求用户自行安装并配置可由 PATH 发现的 Pi coding agent
- Gold Band 不下载、解压、托管或安装 Agent 二进制；非 npx Agent 的安装和 PATH 配置由用户负责，保存后 doctor 负责验证当前配置是否可运行
- 自定义 Agent 复用同一编辑 Sheet；用户填写 Agent ID、icon、命令、参数、环境和 Skills 目录。Agent ID 是持久化与 workflow `provider` 引用使用的稳定唯一标识，创建后不可修改
- 编辑 Sheet 必须显式保存 `catalog / custom / existing` 来源状态，Agent ID 的可编辑性只由来源和创建/编辑生命周期决定，不得根据输入文本是否暂时匹配 Catalog ID 反推；因此自定义输入 `kimi` 后仍可继续填写为 `kimi-for-mine`
- Agent ID 输入必须兼容中文等 IME 组合输入：组合期间保留输入法管理的临时文本，不执行小写化或非法字符过滤；`compositionend` 后再统一规范化为小写字母、数字和连字符，避免回车选词时丢失连字符或光标附近内容
- system prompt 传递能力不在新建或编辑界面展示，也不接受 `ManagedAgentInput` 覆盖；内置 Agent 创建时由 Catalog 写入已验证能力，自定义 Agent 固定为不支持，编辑时保留实例已有的内部能力值
- 新增 Agent 下拉使用 cmdk 的 active 项仅支持键盘确认，不表达业务选中态；下拉刚展开时所有可选项都不显示状态，只有鼠标悬停在具体项上时才显示 hover 背景，避免把“自定义 Agent”误看成默认选中
- 构建与发版脚本从 `https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json` 拉取最新官方 Registry，筛选上述 11 个 Agent，生成并打包 Registry 快照、Catalog 和官方 SVG 图标；离线维护时可显式使用已提交快照重新生成 Catalog
- Catalog 生成必须校验 11 个精选 Agent 全部存在；官方 Registry 缺项时构建失败，禁止静默发布残缺列表
- Kimi Code 的主 Skills 根目录为 `.kimi-code`，兼容读取通用 `.agents`；Gold Band 在这些根目录下统一追加 `skills`
- Pi 的官方 Skills 根目录按作用域不同：全局为 `~/.pi/agent/skills`，项目为 `<project>/.pi/skills`，两端兼容读取 `.agents/skills`；Pi 模板因此默认开启主目录拆分，全局主目录为 `.pi/agent`、项目主目录为 `.pi`

---

## 5. 编辑能力
当前 MVP 编辑项：
- display name
- command
- args
- env
- icon（内置 key、HTTPS/data URI 或应用内绝对资源路径；也可通过系统文件选择器导入不超过 1 MB 的 PNG、JPEG、WebP 或 SVG，导入后以 data URI 随实例配置持久化，不依赖原文件路径；新建自定义 Agent 与空值使用 Gold Band Logo，已有明确保存为 `agent` 的实例继续使用通用 Bot 图标；内置单色 Agent 图标在深色主题下统一反色以保证对比度，品牌彩色、自定义和 Gold Band Logo 保持原色）
- 图标字段的 key、URL 与 data URI 属于持久化实现，不在编辑 Sheet 中暴露文本输入；界面只提供当前图标预览、系统文件选择器和恢复默认 Logo 操作。既有实例中的 URL、data URI、内置 key 与旧 `agent` 值继续正常渲染
- “选择图片”和“恢复默认图标”是一次性命令而不是可选模式，统一使用透明 `ghost` 样式；只有当前 hover 或键盘 `focus-visible` 的命令显示交互反馈，不得让两个操作同时呈现为已选中
- 图标操作区包含多个交互控件，必须使用 `fieldset + legend` 分组，不得复用包裹单个输入框的整行 `<label>`；每个按钮的命中范围只限自身矩形，禁止整行转发到“选择图片”
- “恢复默认图标”的目标随编辑器来源一起初始化：Catalog 新建或既有 Catalog Agent 恢复为该 Catalog 项自己的 `iconKey`，自定义 Agent 恢复为 Gold Band Logo；不得将所有 Agent 统一重置为 Gold Band Logo
- 编辑器以单一生命周期状态统一管理 `open`、来源、Agent ID、表单、文本配置和初始快照；Catalog、自定义和既有实例入口一次性替换完整状态，关闭或保存成功只切换 `open=false`。Sheet 的退出动画期间保留当前 draft，下一次打开再整体初始化，避免内容闪变被误认为抽屉重新打开
- 公共 Sheet 的 overlay 默认跟随 Root 的 `modal` 语义：模态 Sheet 保留遮罩，非模态侧栏默认不渲染遮罩。Agent 编辑器以及其他桌面编辑/详情侧栏统一使用非模态语义；仅窄屏工作区等真正覆盖主界面的 Sheet 保留模态遮罩，避免关闭侧栏时全局页面由暗变亮
- 删除确认框以统一生命周期状态维护 `open + target`；确认、取消或失败只关闭弹窗，不在退出动画期间清空 target，确保 Agent 名称在动画完成前保持稳定
- 主 Agent 目录（默认由全局与项目共用；可通过右侧分裂图标切换为全局主目录和项目主目录）
- 兼容 Agent 目录
- 跨端会话合并与外部会话同步属于暂未对客开放的高级运行能力，当前版本不在 Agent 卡片、新增 Agent 或修改 Agent 界面展示；底层配置、持久化与 runtime 行为继续保留，后续具备自动能力发现与清晰使用场景后再开放

交互：
- 通过右侧 Sheet 编辑
- `command` 在脏状态比较和持久化前统一移除前后空白；只增加或删除命令首尾空格不视为配置修改，前后端配置边界都必须执行同一规范化规则
- `args` 按空格或换行分隔参数，编辑态保留原始多行文本，保存时按空白拆分为真实进程参数，避免一行内多个参数被合成一个参数
- `env` 按 `KEY=VALUE` 输入，编辑态保留原始多行文本，保存时再解析
- 主 Agent 目录允许为空；为空表示相应作用域不参与 Skills 发现、写入和同步。默认未拆分时，全局和项目都使用同一个主目录；点击主目录标题右侧的分裂图标后，按钮呈选中态并显示“全局主目录 / 项目主目录”两个输入框。分裂状态由可选项目主目录字段是否存在表达，不额外持久化布尔状态
- 兼容 Agent 目录按行输入，保存时去除空项、重复项和与任一主目录相同的项；兼容目录在全局和项目作用域都参与 Agent 的 Skill 读取，但不作为 Gold Band 同步写入目标
- 上下文管理中的全局与项目 SKILL 列表必须按作用域扫描所有已配置 Agent 的完整读取目录：全局主目录加兼容目录、项目主目录加兼容目录；未拆分时两端主目录相同。多个 Agent 共享 `.agents` 等同一物理目录时只扫描一次，卡片来源仍显示实际目录名
- SKILL 创建、同步目标对账、冲突检测和同步状态只使用对应作用域的主 Agent 目录，不能因为兼容目录可读而向兼容目录创建文件或软链接
- SKILL 卡片以实体目录为主体，软链接不生成独立卡片；右上角目录标识展示实体实际所在的 Agent 目录，同名但实体目录不同的 SKILL 允许分别展示
- 卡片底部竖线左侧展示读取目录包含该实体目录的全部已配置 Agent；右侧展示不能直读该实体、可在自身主目录创建软链接的 Agent
- 若某个 Agent 已能通过主目录或兼容目录直读实体，但其主目录仍保留历史软链接，则该 Agent 同时出现在左右两侧：左侧表示直读关系，右侧绿色状态图标仅提供删除现有软链接的入口；删除成功后右侧图标消失，左侧图标保留，并且不再提供重新创建冗余软链接的入口
- Agent 新增、删除或目录配置变化后，左右图标集合根据当前配置和实际软链接状态动态重算，不自动清理用户文件系统中的历史软链接
- system prompt 是 Gold Band 内部维护的实例能力而不是 ACP 标准能力发现结果：已验证的 Claude 通过 ACP `session/new` / `session/load` 的 `_meta.systemPrompt.append` 传递稳定 system prompt；其他内置与自定义 Agent 默认关闭，并仅在新会话首个 user prompt 内嵌隐藏稳定上下文，恢复会话不重复注入
- “支持跨端会话合并”与“启用外部会话同步”分开保存；能力关闭时启用值必须在前后端同时归一化为 `false`
- 隐藏期间编辑其他 Agent 字段必须原样保留已有跨端能力与同步配置，不得因为界面不可见而静默关闭或改写；新建实例继续采用 Catalog 或自定义 Agent 的既有默认值
- 修改 Sheet 必须基于规范化后的持久化配置判断脏状态；未修改时“保存”按钮禁用，提交入口也不得调用更新接口或触发自动诊断。参数仅改变空格/换行、环境变量仅调整行顺序但规范化结果一致时，不视为配置修改
- 保存成功后立即清空当前 agent 的旧诊断状态，并由桌面端后台自动触发一次环境诊断；保存接口既不等待本次 doctor，也不等待正在执行的手动或周期 doctor，持久化成功后立即关闭编辑 Sheet 并提示“配置已保存，正在后台诊断”
- 自动诊断运行期间，卡片沿用“诊断中”加载状态并暂时禁用修改、删除和重复诊断；诊断完成后通过桌面事件刷新全局 Agent registry，并按健康或异常结果展示数秒横幅
- 新增、编辑或删除某个 agent 时，只允许影响该 agent 自身的诊断缓存；其他已诊断 agent 的状态与最近检测时间必须保留

配置持久化：
- `settings.json` 使用 `settingsSchemaVersion` 标记结构版本；缺少版本号的旧配置视为版本 `0`
- 桌面端、CLI、MCP 和应用服务统一通过同一个设置加载入口读取配置；旧 Agent ID、`skillsDirOverride` 和缺失的目录字段只在版本升级时迁移，并通过原子写一次性写回当前版本
- settings schema v2 将已保存参数中的 `@zed-industries/codex-acp` 包规格一次性替换为 `@agentclientprotocol/codex-acp@latest`；其他自定义 Codex 命令和参数保持不变
- settings schema v3 为旧实例一次性补齐 icon、system prompt 传递方式和跨端会话合并能力；旧 Claude 继承已知的 `_meta.systemPrompt.append` 能力，其他旧 Agent 默认不支持。曾经启用外部会话同步的实例同步标记为具备该能力，以保留既有行为
- 当前版本配置在后续启动时只解析和检查版本，不重复迁移或写回；未来版本高于当前程序支持范围时必须明确报错
- 配置解析或迁移失败不得静默回退为默认设置，避免启动时内存配置与保存时磁盘配置不一致

---

## 6. 诊断能力
每个 agent card 提供：
- 手动“环境诊断”按钮
- 诊断状态图标
- 最近检测时间（展示为本地系统时区 `YYYY-MM-DD HH:MM:SS`）
- 错误原因（如果有）
- doctor 失败时在诊断状态旁显示问号帮助入口；该入口统一使用随主题变化的浅色 shadcn/ui `Tooltip`，悬浮或聚焦即可展示错误原因与 ACP Registry 配置帮助，不使用自定义 tooltip 大面板；提示内容仅包含参考 ACP Registry 配置命令、参数、环境变量、网络和认证状态，其中 ACP Registry 为外链 `https://agentclientprotocol.com/get-started/registry`，点击提示内链接时通过系统默认浏览器打开，不在卡片内展开具体下载步骤
- 诊断运行中按钮展示圆形加载动效
- 诊断完成后根据结果显示数秒横幅：正常为成功横幅，异常为异常横幅并展示原因
- 横幅在浅色模式下必须保证文案可读性，成功态文案与图标应复用主题语义成功色 token，不允许在页面里硬编码浅绿色并导致低对比度问题

后台能力：
- 桌面端启动后自动执行诊断
- 后台每 60 秒自动诊断一次当前 workspace 下已配置 agent
- 新增或修改 Agent 配置保存成功后，后台立即自动诊断该 agent 一次，不要求用户再手动点击“环境诊断”
- 手动诊断、保存后自动诊断、周期诊断和命令目录刷新共享同一个 doctor 运行边界；配置持久化使用独立的短时提交边界，不能被长时间 doctor 阻塞。诊断提交结果前必须再次校验 Agent 配置版本，旧配置的诊断结果不得覆盖新配置；连续保存产生的同 agent 自动诊断请求必须按版本合并并最终只诊断最新版。同一 Agent 禁止并发启动重复 adapter；全量周期诊断允许不同 Agent 并行，但每个 Agent 必须使用独立的 `doctor/acp/<agent-id>` attempt 目录和 `provider.pid`，任何诊断只能清理自己所属的目录
- 手动诊断、保存后自动诊断和命令目录刷新只执行一次 doctor，不自动重试。后台每 60 秒周期诊断首次失败时在同一轮内自动重试一次；首次失败不得提前发布或持久化为异常，重试仍失败才以第二次错误作为最终诊断结果并展示原因
- 手动诊断和自动诊断都必须在诊断结束、初始化失败、超时或客户端关闭时关闭 ACP adapter 进程树
- 诊断对当前已配置的 ACP adapter 通用执行，不再限定 Claude；首次运行 npx 或本地二进制 adapter 可能需要安装依赖，耗时可达到 1 分钟以上
- ACP adapter、doctor 共用 Rust 进程环境解析层，不得在调用点按 Kimi、Cursor、包管理器或固定安装目录增加特判。Windows 的稳定优先级为“Agent 显式配置 PATH → 当前桌面进程 PATH → 用户注册表 PATH → 系统注册表 PATH → 平台通用目录”，每次创建 ACP 进程前通过注册表 API 动态读取 `HKCU\Environment\Path` 与 `HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment\Path`，展开 `%VAR%` 后按大小写不敏感去重，因此用户在应用启动后安装并正式写入用户/系统 PATH 的 Agent 无需重启 Gold Band。macOS / Linux 的稳定优先级为“Agent 显式配置 PATH → 用户登录 Shell PATH → 当前桌面进程 PATH → 平台通用目录”，首次解析当前 Unix 账户的默认 Shell，以 `-ilc` 登录交互模式读取环境并在 2 秒总超时内回收异常 Shell；成功或失败结果均按应用进程生命周期缓存。Shell profile 输出通过边界标记解析，只采纳其中的 `PATH`，并禁用 Oh My Zsh 自动更新与 tmux 自动启动；解析失败时继续使用当前进程 PATH。Unix 按大小写敏感语义去重，平台通用目录仅补全 `~/.nvm/versions/node/*/bin`、`~/.local/bin`、`~/.cargo/bin`、`~/.volta/bin`、`/opt/homebrew/bin`、`/usr/local/bin` 等非 Agent 专属位置。登录 Shell/注册表读取和 PATH 合并只发生在 doctor / ACP 进程创建边界，不进入消息处理热路径
- Windows 裸命令在每个 PATH 目录中只按 `.exe`、`.com`、`.cmd`、`.bat` 顺序选择可由后台进程稳定启动的候选，不自动选择 `.ps1`，也不把 npm 同目录生成的无扩展名 Unix shell shim 当作 Win32 应用；用户显式填写带扩展名命令时只按原名查找。必须使用 PowerShell 脚本的自定义 Agent 应显式配置 `pwsh` / `powershell` 与 `-NoProfile -File <script.ps1>` 参数
- 若 adapter 启动失败，诊断原因必须保留底层 OS 错误文本，例如 `No such file or directory (os error 2)`，不能只显示泛化的“failed to start ACP adapter”
- 当前固定参考官方 Registry 中的 Claude、Codex、Cursor、Gemini、CodeBuddy、Goose、Qwen Code、OpenCode、Kimi Code、Amp、Pi 十一类精选 Agent，同时允许任意合法自定义 ACP Agent
- 诊断 initialize 设置 5 分钟超时，超时视为异常诊断并返回页面，不允许阻塞客户端
- 诊断结果除健康状态外，还要缓存 agent 返回的 `modes` / `configOptions` 能力摘要，供工作流编辑器直接复用
- 诊断缓存需要持久化到当前 workspace 的本地运行时目录，客户端重启后仍可直接为节点展示可选权限模式，不要求用户每次重新手动诊断

---

## 7. 与 workflow 的关系
Agent 管理页不是 workflow 编辑器，但它决定 workflow 里声明的 agent type 是否可执行。

当前约束：
- workflow 节点中的 `provider` 字段表示稳定的 managed Agent ID
- workflow 画布、Agent 选择器、会话标题和侧栏统一从当前 `AgentRegistryVm.agents` 实例读取 display name 与 icon，不允许维护按 provider ID 推导图标的内置白名单；因此 Catalog Agent、自定义 Agent、用户上传图标和默认通用图标共享同一展示语义
- 创建任务与工作流编辑器的节点 Agent 下拉展示全部已配置 Agent；最近一次 doctor 成功的 Agent 可选择，未运行 doctor、doctor 失败或诊断缓存缺失的 Agent 保留展示但禁用，并展示诊断失败原因。是否来自内置 Catalog 不参与可用性判断
- 已有节点引用的 Agent 诊断失败后必须保留原选择，不得把节点表现为“未关联 Agent”；该工作流不能保存或启动，后端命令入口继续拦截，用户可到 Agent 管理页手动重试诊断
- 若节点引用的 agent type 未在 Agent 管理页中配置或未通过 doctor，则 workflow 校验失败
- workflow 节点权限模式必须来自该 agent 最近一次 doctor 缓存的 `supportedModes`；切换 agent 时不继承旧 agent 的权限模式
- 权限模式与 Profile 是正交配置：权限模式通过 ACP `session/set_mode` 或 mode 类 `session/set_config_option` 控制工具授权，Profile 提示词继续约束节点职责和允许产物。把规划节点实时切换为完全授权只表示 Agent 可以执行该权限模式允许的工具，不会解除 `pf-builtin-plan` 的“只规划、不修改代码”职责；需要实施时应切换/新增开发 Profile 节点
- 节点详情页应展示当前节点绑定的 agent type，便于确认执行来源

---

## 8. 一句话总结
> Agent 管理页解决的是“这个 Agent ID 在当前 workspace 里怎么跑、从哪些 Agent 目录发现 Skill、是否健康”；节点执行仍然由 workflow 显式声明 `provider` 决定。
