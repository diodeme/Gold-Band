# Agent 管理页

## 1. 一句话定义
Agent 管理页负责维护当前桌面 workspace 可用的 agent type 配置，并提供诊断、编辑与删除能力。

---

## 2. 页面目标
当前桌面端需要把“节点声明用哪个 agent”和“这个 agent 实际怎么执行”分开：
- workflow 节点通过 `provider` 显式声明 agent type
- Agent 管理页负责维护这个 type 的执行命令、参数、环境变量和诊断状态

当前规则：
- Worker / Verify 节点必须显式声明 `provider`
- 当前不提供默认 Claude 兜底
- 当前同一 agent type 只能配置一份实例

---

## 3. 页面结构

```text
Page Header
- 标题 / 副标题
- 刷新
- 新增 Agent（下拉）

Agent Cards
- icon
- display name
- agent type
- command / args / env 摘要
- 诊断状态 / 最近检测时间（本地系统时区 `YYYY-MM-DD HH:MM:SS`）
- 诊断 / 修改 / 删除

布局要求：
- agent card 内容与卡片边缘保持稳定左右内边距，不允许内容贴边
- 列表默认优先采用高密度多列布局；桌面常见宽度下应尽量一行展示 3 张卡片，再逐级退化到 2 列或 1 列
- 不减少命令、参数、环境变量、最近检测等关键信息项，但需通过更紧凑的标题区、信息区和操作区压缩卡片高度
- 编辑 Sheet 头部、表单区和底部操作区需要保持统一左右内边距
```

---

## 4. 新增 Agent
新增按钮使用下拉菜单，列表来自 ACP registry 固定支持集：
- Claude：`claude-acp`
- Codex：`codex-acp`
- Cursor：`cursor`
- Gemini：`gemini`
- OpenCode：`opencode`

限制：
- 已配置过的 agent type 不可重复新增
- 新增时预填 registry 推荐命令、参数和 display name，用户可按本机安装路径调整；npx 类 agent 使用 registry package，Cursor/OpenCode 默认走 PATH 中的 `cursor-agent acp` / `opencode acp`
- agent 图标源文件维护在 `docs\gold-band\资源\icon`，应用实际打包路径为 `web\public\agent-icons`，由 Vite 复制进 `web\dist` 后随 Tauri 应用打包

---

## 5. 编辑能力
当前 MVP 编辑项：
- display name
- command
- args
- env

交互：
- 通过右侧 Sheet 编辑
- `args` 按空格或换行分隔参数，编辑态保留原始多行文本，保存时按空白拆分为真实进程参数，避免一行内多个参数被合成一个参数
- `env` 按 `KEY=VALUE` 输入，编辑态保留原始多行文本，保存时再解析
- 保存只更新配置并清空当前 agent 的旧诊断状态，不同步触发环境诊断，避免保存流程被诊断进程阻塞
- 新增、编辑或删除某个 agent 时，只允许影响该 agent 自身的诊断缓存；其他已诊断 agent 的状态与最近检测时间必须保留

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
- 手动诊断和自动诊断都必须在诊断结束、初始化失败、超时或客户端关闭时关闭 ACP adapter 进程树
- 诊断对当前已配置的 ACP adapter 通用执行，不再限定 Claude；首次运行 npx 或本地二进制 adapter 可能需要安装依赖，耗时可达到 1 分钟以上
- 桌面端启动 ACP adapter 前需要自动补全常见用户 bin 目录到子进程 PATH，例如 `~/.nvm/versions/node/*/bin`、`~/.local/bin`、`~/.cargo/bin`、`~/.opencode/bin`、`/opt/homebrew/bin`、`/usr/local/bin`，避免 macOS GUI 进程未继承 shell PATH 时 `npx`、`node`、`claude`、`codex` 无法启动
- 若 adapter 启动失败，诊断原因必须保留底层 OS 错误文本，例如 `No such file or directory (os error 2)`，不能只显示泛化的“failed to start ACP adapter”
- 当前固定参考 `https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json` 中的 `claude-acp`、`codex-acp`、`cursor`、`gemini`、`opencode` 五类 registry agent
- 诊断 initialize 设置 5 分钟超时，超时视为异常诊断并返回页面，不允许阻塞客户端
- 诊断结果除健康状态外，还要缓存 agent 返回的 `modes` / `configOptions` 能力摘要，供工作流编辑器直接复用
- 诊断缓存需要持久化到当前 workspace 的本地运行时目录，客户端重启后仍可直接为节点展示可选权限模式，不要求用户每次重新手动诊断

---

## 7. 与 workflow 的关系
Agent 管理页不是 workflow 编辑器，但它决定 workflow 里声明的 agent type 是否可执行。

当前约束：
- workflow 节点中的 `provider` 字段表示 managed agent type
- 创建任务与工作流编辑器的节点 Agent 下拉只展示已配置、当前支持且最近一次 doctor 成功的 agent card
- 未运行 doctor、doctor 失败或诊断缓存缺失的 agent 不能被工作流选择，保存工作流时也会被命令入口拦截
- 若节点引用的 agent type 未在 Agent 管理页中配置或未通过 doctor，则 workflow 校验失败
- workflow 节点权限模式必须来自该 agent 最近一次 doctor 缓存的 `supportedModes`；切换 agent 时不继承旧 agent 的权限模式
- 节点详情页应展示当前节点绑定的 agent type，便于确认执行来源

---

## 8. 一句话总结
> Agent 管理页解决的是“这个 agent type 在当前 workspace 里怎么跑、是否健康”；节点执行仍然由 workflow 显式声明 `provider` 决定。
