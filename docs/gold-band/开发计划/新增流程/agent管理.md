现在应用程序的侧边栏是任务编排、知识库、模型管理
你现在先新增个agent管理吧
agent管理主要是负责管理支持接入的ACP agent
当前改为参考 ACP registry 固定支持 `claude-acp`、`codex-acp`、`cursor`、`gemini`、`opencode` 五类 agent
agent管理页面主要就是agent卡片和新增agent按钮
agent卡片支持删除、修改、环境诊断操作（检查agent环境是否正常，提供手动检测能力，后台每1分钟自动检测一次agent环境），并显示agent的诊断状态（最好用对应图标）；doctor 失败时在状态旁显示问号帮助入口，该帮助入口统一使用随主题变化的浅色 shadcn/ui `Tooltip` 展示错误原因与配置帮助，悬浮或聚焦即可出现；提示参考 ACP Registry 配置命令、参数、环境、网络和认证状态，ACP Registry 链接到 `https://agentclientprotocol.com/get-started/registry`，点击后通过系统默认浏览器打开。卡片内容需要有稳定左右内边距；最近检测时间展示为本地系统时区 `YYYY-MM-DD HH:MM:SS`；手动诊断运行中显示圆形加载动效，完成后根据结果显示数秒成功或异常横幅；成功态横幅与成功状态图标需复用主题 success token，避免页面硬编码颜色；诊断命令 `npx -y @agentclientprotocol/claude-agent-acp@latest` 用于启动 Claude ACP adapter，首次运行可能通过 npm 下载依赖而耗时 1 分钟以上；诊断 initialize 最多等待 5 分钟，结束、失败、超时或客户端关闭都必须退出诊断进程树，不能阻塞客户端
补充诊断环境要求：
- 桌面端在启动 ACP adapter 之前，需要为子进程自动补全常见用户 bin 目录到 PATH，例如 `~/.nvm/versions/node/*/bin`、`~/.local/bin`、`~/.cargo/bin`、`~/.opencode/bin`、`/opt/homebrew/bin`、`/usr/local/bin`，避免 macOS GUI 进程未继承 shell PATH 时 `npx`、`node`、`claude`、`codex` 启动失败
- 若 adapter 启动失败，doctor 结果必须保留底层 OS 错误文本，例如 `No such file or directory (os error 2)`，不能只显示泛化失败文案
新增agent按钮点击后，可以下拉栏选择 `claude-acp`、`codex-acp`、`cursor`、`gemini`、`opencode`；新增表单按 registry 推荐命令和参数预填，npx 类 agent 使用 registry package，Cursor/OpenCode 默认使用 PATH 中的 `cursor-agent acp` / `opencode acp`，已新增过的类型不可重复新增
agent需要有对应icon标识，参考 `docs\gold-band\资源\icon` 目录；应用打包实际读取 `web\public\agent-icons`，Cursor 图标也必须同步复制到该目录
新增agent时，已经新增过的agent类型，不能重复新增
agent配置需要做持久化管理；修改 Sheet 的参数和环境变量使用可换行的多行编辑区，编辑时不即时吞掉空行或换行；参数保存时按空格或换行拆分，环境变量保存时按行解析；保存配置不自动执行环境诊断，只清空当前 agent 的旧诊断状态，避免保存卡死；新增、编辑或删除某个 agent 时，不允许把其他已诊断 agent 一并回退成未诊断

补充实现约束：
- worker 节点中的 `provider` 字段显式声明 agent type，当前不提供默认 claude 兜底
- 当前 agent type 直接作为 registry key 使用，因此同一类型只能维护一份配置
- 节点详情页需要展示当前节点声明的 agent type，便于确认执行来源
- 工作流创建、修改和模板保存时，Agent 下拉只允许选择已配置且最近一次 doctor 成功的 agent；未诊断或诊断失败的 agent 不能进入 workflow
- workflow 节点的权限模式只能从当前 agent doctor 返回的 `supportedModes` 中选择；切换 agent 时清空旧权限模式，不做跨 agent 权限模式映射
