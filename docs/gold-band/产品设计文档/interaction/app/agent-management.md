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
- 标题 / 副标题
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
- agent card 内容与卡片边缘保持稳定左右内边距，不允许内容贴边
- 列表默认优先采用高密度多列布局；桌面常见宽度下应尽量一行展示 3 张卡片，再逐级退化到 2 列或 1 列
- 不减少命令、参数、环境变量、最近检测等关键信息项，但需通过更紧凑的标题区、信息区和操作区压缩卡片高度
- 编辑 Sheet 头部、表单区和底部操作区需要保持统一左右内边距
```

---

## 4. 新增 Agent
新增按钮使用下拉菜单，列表来自 Gold Band 内置 Agent preset registry：
- Claude：`claude-acp`
- Codex：`codex-acp`
- Cursor：`cursor`
- Gemini：`gemini`
- OpenCode：`opencode`

限制：
- 已配置过的 Agent ID 不可重复新增
- preset 同时提供稳定 ID、名称、图标、推荐命令、参数、主 Agent 目录和兼容 Agent 目录；新增时把完整默认配置写入 `ManagedAgentConfig`，运行时不再根据 Agent ID 推导目录
- 新增时预填 registry 推荐命令、参数和 display name，用户可按本机安装路径调整；npx 类 agent 使用 registry package，Cursor/OpenCode 默认走 PATH 中的 `cursor-agent acp` / `opencode acp`
- agent 图标源文件维护在 `docs\gold-band\资源\icon`，应用实际打包路径为 `web\public\agent-icons`，由 Vite 复制进 `web\dist` 后随 Tauri 应用打包

---

## 5. 编辑能力
当前 MVP 编辑项：
- display name
- command
- args
- env
- 主 Agent 目录
- 兼容 Agent 目录
- 外部会话同步（Beta，默认关闭）

交互：
- 通过右侧 Sheet 编辑
- `command` 在脏状态比较和持久化前统一移除前后空白；只增加或删除命令首尾空格不视为配置修改，前后端配置边界都必须执行同一规范化规则
- `args` 按空格或换行分隔参数，编辑态保留原始多行文本，保存时按空白拆分为真实进程参数，避免一行内多个参数被合成一个参数
- `env` 按 `KEY=VALUE` 输入，编辑态保留原始多行文本，保存时再解析
- 主 Agent 目录不能为空；Gold Band 在该目录下统一追加 `skills`，该目录同时用于 Skill 读取、写入和同步
- 兼容 Agent 目录按行输入，保存时去除空项、重复项和与主目录相同的项；兼容目录只参与 Agent 的 Skill 读取，不作为 Gold Band 同步写入目标
- 上下文管理中的全局与项目 SKILL 列表必须扫描所有已配置 Agent 的完整读取目录，即主 Agent 目录加兼容 Agent 目录；多个 Agent 共享 `.agents` 等同一物理目录时只扫描一次，卡片来源仍显示实际目录名
- SKILL 创建、同步目标对账、冲突检测和同步状态只使用主 Agent 目录，不能因为兼容目录可读而向兼容目录创建文件或软链接
- SKILL 卡片以实体目录为主体，软链接不生成独立卡片；右上角目录标识展示实体实际所在的 Agent 目录，同名但实体目录不同的 SKILL 允许分别展示
- 卡片底部竖线左侧展示读取目录包含该实体目录的全部已配置 Agent；右侧展示不能直读该实体、可在自身主目录创建软链接的 Agent
- 若某个 Agent 已能通过主目录或兼容目录直读实体，但其主目录仍保留历史软链接，则该 Agent 同时出现在左右两侧：左侧表示直读关系，右侧绿色状态图标仅提供删除现有软链接的入口；删除成功后右侧图标消失，左侧图标保留，并且不再提供重新创建冗余软链接的入口
- Agent 新增、删除或目录配置变化后，左右图标集合根据当前配置和实际软链接状态动态重算，不自动清理用户文件系统中的历史软链接
- 外部会话同步标题右侧展示紧凑 `Beta` Badge 和可聚焦问号 Tooltip；Tooltip 解释“同步同一个 Session 在其他客户端中发生过的对话”，常驻说明则明确提示仅在确认该 Agent 支持跨客户端共享同一会话上下文时开启，否则可能造成历史顺序或上下文理解错误
- 修改 Sheet 必须基于规范化后的持久化配置判断脏状态；未修改时“保存”按钮禁用，提交入口也不得调用更新接口或触发自动诊断。参数仅改变空格/换行、环境变量仅调整行顺序但规范化结果一致时，不视为配置修改
- 保存成功后立即清空当前 agent 的旧诊断状态，并由桌面端后台自动触发一次环境诊断；保存接口既不等待本次 doctor，也不等待正在执行的手动或周期 doctor，持久化成功后立即关闭编辑 Sheet 并提示“配置已保存，正在后台诊断”
- 自动诊断运行期间，卡片沿用“诊断中”加载状态并暂时禁用修改、删除和重复诊断；诊断完成后通过桌面事件刷新全局 Agent registry，并按健康或异常结果展示数秒横幅
- 新增、编辑或删除某个 agent 时，只允许影响该 agent 自身的诊断缓存；其他已诊断 agent 的状态与最近检测时间必须保留

配置持久化：
- `settings.json` 使用 `settingsSchemaVersion` 标记结构版本；缺少版本号的旧配置视为版本 `0`
- 桌面端、CLI、MCP 和应用服务统一通过同一个设置加载入口读取配置；旧 Agent ID、`skillsDirOverride` 和缺失的目录字段只在版本升级时迁移，并通过原子写一次性写回当前版本
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
- 手动诊断、保存后自动诊断、周期诊断和命令目录刷新共享同一个 doctor 运行边界；配置持久化使用独立的短时提交边界，不能被长时间 doctor 阻塞。诊断提交结果前必须再次校验 Agent 配置版本，旧配置的诊断结果不得覆盖新配置；连续保存产生的同 agent 自动诊断请求必须按版本合并并最终只诊断最新版，禁止并发清理同一 `doctor/acp` 目录或启动重复 adapter
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
- workflow 节点中的 `provider` 字段表示稳定的 managed Agent ID
- 创建任务与工作流编辑器的节点 Agent 下拉只展示已配置、当前支持且最近一次 doctor 成功的 agent card
- 未运行 doctor、doctor 失败或诊断缓存缺失的 agent 不能被工作流选择，保存工作流时也会被命令入口拦截
- 若节点引用的 agent type 未在 Agent 管理页中配置或未通过 doctor，则 workflow 校验失败
- workflow 节点权限模式必须来自该 agent 最近一次 doctor 缓存的 `supportedModes`；切换 agent 时不继承旧 agent 的权限模式
- 节点详情页应展示当前节点绑定的 agent type，便于确认执行来源

---

## 8. 一句话总结
> Agent 管理页解决的是“这个 Agent ID 在当前 workspace 里怎么跑、从哪些 Agent 目录发现 Skill、是否健康”；节点执行仍然由 workflow 显式声明 `provider` 决定。
