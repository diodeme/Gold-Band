现在应用程序的侧边栏是任务编排、知识库、模型管理
你现在先新增个agent管理吧
agent管理主要是负责管理支持接入的ACP agent
目前可以先仅支持claude code
agent管理页面主要就是agent卡片和新增agent按钮
agent卡片支持删除、修改、环境诊断操作（检查agent环境是否正常，提供手动检测能力，后台每1分钟自动检测一次agent环境），并显示agent的诊断状态（最好用对应图标）。卡片内容需要有稳定左右内边距；最近检测时间展示为本地系统时区 `YYYY-MM-DD HH:MM:SS`；手动诊断运行中显示圆形加载动效，完成后根据结果显示数秒成功或异常横幅；诊断命令 `npx -y @agentclientprotocol/claude-agent-acp@latest` 用于启动 Claude ACP adapter，首次运行可能通过 npm 下载依赖而耗时 1 分钟以上；诊断 initialize 最多等待 5 分钟，结束、失败、超时或客户端关闭都必须退出诊断进程树，不能阻塞客户端
新增agent按钮点击后，可以下拉栏选择agent类型（目前仅支持claude code，但是列表可以包括codex-cli、opencode、gemini-cli，只是标注待支持）
agent需要有对应icon标识，参考docs\gold-band\资源\icon目录
新增agent时，已经新增过的agent类型，不能重复新增
agent配置需要做持久化管理；修改 Sheet 的参数和环境变量使用可换行的多行编辑区，编辑时不即时吞掉空行或换行；参数保存时按空格或换行拆分，环境变量保存时按行解析；保存配置不自动执行环境诊断，只清空旧诊断状态，避免保存卡死

补充实现约束：
- worker / verify 节点中的 `provider` 字段显式声明 agent type，当前不提供默认 claude 兜底
- 当前 agent type 直接作为 registry key 使用，因此同一类型只能维护一份配置
- 节点详情页需要展示当前节点声明的 agent type，便于确认执行来源