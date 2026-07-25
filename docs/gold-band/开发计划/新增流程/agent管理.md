现在应用程序的侧边栏是任务编排、知识库、模型管理
你现在先新增个agent管理吧
agent管理主要是负责管理支持接入的ACP agent
当前改为参考 ACP registry 固定支持 `claude-acp`、`codex-acp`、`cursor`、`gemini`、`opencode` 五类 agent
agent管理页面主要就是agent卡片和新增agent按钮
agent卡片支持删除、修改、环境诊断操作（检查agent环境是否正常，提供手动检测能力，后台每1分钟自动检测一次agent环境），并显示agent的诊断状态（最好用对应图标）；doctor 失败时在状态旁显示问号帮助入口，该帮助入口统一使用随主题变化的浅色 shadcn/ui `Tooltip` 展示错误原因与配置帮助，悬浮或聚焦即可出现；提示参考 ACP Registry 配置命令、参数、环境、网络和认证状态，ACP Registry 链接到 `https://agentclientprotocol.com/get-started/registry`，点击后通过系统默认浏览器打开。卡片内容需要有稳定左右内边距；最近检测时间展示为本地系统时区 `YYYY-MM-DD HH:MM:SS`；手动诊断运行中显示圆形加载动效，完成后根据结果显示数秒成功或异常横幅；成功态横幅与成功状态图标需复用主题 success token，避免页面硬编码颜色；诊断命令 `npx -y @agentclientprotocol/claude-agent-acp@latest` 用于启动 Claude ACP adapter，首次运行可能通过 npm 下载依赖而耗时 1 分钟以上；诊断 initialize 最多等待 5 分钟，结束、失败、超时或客户端关闭都必须退出诊断进程树，不能阻塞客户端
补充诊断环境要求：
- 桌面端在启动 ACP adapter 之前，需要为子进程自动补全常见用户 bin 目录到 PATH，例如 `~/.nvm/versions/node/*/bin`、`~/.local/bin`、`~/.cargo/bin`、`~/.opencode/bin`、`/opt/homebrew/bin`、`/usr/local/bin`，避免 macOS GUI 进程未继承 shell PATH 时 `npx`、`node`、`claude`、`codex` 启动失败
- “使用本地 Claude”只影响 Claude ACP adapter 的 `CLAUDE_CODE_EXECUTABLE` 注入。Windows 上需要同时兼容原生安装和 npm 安装：原生安装优先使用 PATH 中的 `claude.exe`；npm 安装若 PATH 目录暴露 `claude.cmd` shim，则读取 `.cmd` wrapper 内容并解析其实际指向的 native `.exe`，例如 `%dp0%\node_modules\@anthropic-ai\claude-code\bin\claude.exe`，不能把 extensionless `claude` shell shim 传给 adapter，也不能依赖固定 npm prefix 拼接路径；若找不到原生 binary，则不注入环境变量。macOS / Linux 继续按 PATH 查找可执行 `claude`；Unix npm shim 本身是可执行脚本，不需要像 Windows 一样解析 `.cmd` wrapper。
- 本地 Claude 注入调用点必须复用统一解析函数，并通过单元测试覆盖 Windows npm `.cmd` 内容反查场景，避免后续合并时只保留 helper/测试却把 adapter 启动调用点回退成仅查 `claude.exe` 或固定目录拼接。
- 项目级 `configs/app-config.toml` 提供 `requireLocalClaudeExecutable` 诊断开关，默认关闭。开启后，“使用本地 Claude”但未解析出 native executable 时直接让 doctor / 会话启动失败，不再进入 `claude-agent-acp` / Claude Agent SDK 内部 fallback，用于验证本地发现逻辑；临时排障也可用环境变量 `GOLD_BAND_REQUIRE_LOCAL_CLAUDE=1` 覆盖开启。
- 若 adapter 启动失败，doctor 结果必须保留底层 OS 错误文本，例如 `No such file or directory (os error 2)`，不能只显示泛化失败文案
新增agent按钮点击后，可以下拉栏选择 `claude-acp`、`codex-acp`、`cursor`、`gemini`、`opencode`；新增表单按 registry 推荐命令和参数预填，npx 类 agent 使用 registry package，Cursor/OpenCode 默认使用 PATH 中的 `cursor-agent acp` / `opencode acp`，已新增过的类型不可重复新增
agent需要有对应icon标识，参考 `docs\gold-band\资源\icon` 目录；应用打包实际读取 `web\public\agent-icons`，Cursor 图标也必须同步复制到该目录
新增agent时，已经新增过的agent类型，不能重复新增
agent配置需要做持久化管理；修改 Sheet 的参数和环境变量使用可换行的多行编辑区，编辑时不即时吞掉空行或换行；参数保存时按空格或换行拆分，环境变量保存时按行解析；保存成功后只清空当前 agent 的旧诊断状态，并由后端后台自动诊断该 agent 一次，保存接口不得等待本次或已经运行中的 doctor，持久化完成后前端立即关闭 Sheet 并提示“配置已保存，正在后台诊断”；自动诊断期间卡片显示诊断中并禁用重复修改、删除和诊断，完成后通过桌面事件刷新全局 Agent registry；新增、编辑或删除某个 agent 时，不允许把其他已诊断 agent 一并回退成未诊断

修改 Sheet 需要保存打开时的规范化配置快照，并与当前按真实持久化规则解析后的配置比较。无修改时禁用保存按钮，提交函数也必须二次拦截，不能调用 `update_agent` 或触发 doctor；参数空白布局变化、环境变量行顺序变化等规范化后等价的输入不算修改。

Agent `command` 在前端构建保存参数、脏状态比较以及后端 `ManagedAgentInput` 转配置时统一执行首尾 `trim`。仅修改命令前后空格不算配置变化，也不能触发保存和自动诊断；真实命令内容变化仍正常保存。

Agent 修改 Sheet 的外部会话同步开关属于 Beta 能力，默认关闭。标题右侧使用 shadcn/ui `Badge` 展示 `Beta`，并使用可聚焦的问号 `Tooltip` 解释“同步同一个 Session 在其他客户端中发生过的对话”；常驻说明文案必须明确：仅在确认该 Agent 支持跨客户端共享同一会话上下文时开启，否则可能造成历史顺序或上下文理解错误。

诊断生命周期补充：手动诊断、保存后自动诊断和每分钟周期诊断共享统一 doctor 运行边界；配置保存使用独立短时提交锁，不能等待 doctor。诊断完成写入前再次比对配置版本，版本已变化则丢弃旧结果并继续处理最新版；同一 agent 的连续自动诊断请求按版本合并。接口层单元测试需要固化保存提交不受运行中 doctor 阻塞、自动诊断请求去重与完成后可再次入队的行为。

补充实现约束：
- worker 节点中的 `provider` 字段显式声明 agent type，当前不提供默认 claude 兜底
- 当前 agent type 直接作为 registry key 使用，因此同一类型只能维护一份配置
- 节点详情页需要展示当前节点声明的 agent type，便于确认执行来源
- 工作流创建、修改和模板保存时，Agent 下拉只允许选择已配置且最近一次 doctor 成功的 agent；未诊断或诊断失败的 agent 不能进入 workflow
- workflow 节点的权限模式只能从当前 agent doctor 返回的 `supportedModes` 中选择；切换 agent 时清空旧权限模式，不做跨 agent 权限模式映射
