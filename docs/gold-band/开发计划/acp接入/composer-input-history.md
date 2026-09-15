# 会话输入历史翻阅开发与验收

## 需求与设计判断

目标是在会话输入框用上下箭头复用本会话用户原文，最早一条向上循环回到最新一条；向下越过最新一条恢复进入翻阅前的完整草稿。详细交互以产品设计 `interaction/app/conversational-runtime.md` 的“Composer 输入历史翻阅”为真源。

现有 prompt-kit 键盘入口和草稿管理可以承载此能力，属于现有设计上的功能补全。数据边界需要补充：历史 UI 仅有当前窗口，且可见 user 事件有时包含组装后的运行时 prompt，不能直接作为输入历史源。沿用 Timeline 索引定位读取，并在生产事件时记录原文来源标记；不引入第二份消息模型，不推测旧记录是否为原文。

## 数据、接口与生命周期

- 消息持久化事实：Timeline content、promptId、startedSeq、generation 和 originalUserText 来源元数据；既有索引仅增加可派生 composerTextBytes 摘要，不复制正文。
- 输入事实：既有 AcpComposerDraft。历史 reader 仅保存可丢弃的有界摘要与少量正文，hook 只维护游标、请求资格及局部加载状态。
- 查询：两个只读 Tauri 接口 list_composer_history / get_composer_history_text，使用完整 attempt locator 和结构化错误码，在 blocking pool 内访问索引。
- 每次翻阅冻结 head；草稿编辑、上下文加入、提交、会话切换、禁用和卸载使未完成查询失效。历史文字作为 composer 内临时投影，编辑或发送时采用为纯文字草稿；发送使用原提交链路。

## 实施与验证

- [x] 后端按页摘要与单条正文接口、游标校验、原文来源过滤。
- [x] RuntimeApi desktop/browser 适配与 composer 局部键盘控制。
- [x] 跨页循环、同文不同身份、无历史/单条、多行、输入法、选区、异步取消接口测试。
- [x] 后端新增排除项与一万条记录读取验证。
- [x] 桌面 cargo check、前端生产构建、真实浏览器 composer 交互验证。
- [x] 索引摘要字段修改后的共享 Timeline 回归和最终桌面编译复核：32 项通过，1 项需外部真实 fixture 的既有测试按默认配置跳过；最终 cargo check 通过。

## 方案自评审

过度设计：复用现有组件与数据源，没有新增依赖、全局状态、数据库、缓存服务、队列或跨层 identity。来源标记表达原有事件类型无法区分的“content 是否为独立用户原文”，不承担生命周期状态。

性能：只在按键时查询；前端每次最多 40 条摘要、3 条正文且约 1 MiB 缓存，单请求在途，无后台预取。后端复用通用 item reader 的 4 项、32 MiB 有界 LRU，在读取投影中维护有序输入集合、prompt identity 去重位置和正文 locator；append 只增量回放新增尾记录。热分页按游标范围访问页大小同阶的候选，正文只读取一条 JSONL 记录；冷启动、淘汰或索引失效仍允许一次 O(N) 加载或重建。以一万条合格输入作为测量规模，验证页大小上限、无全量正文响应、预热后完整索引加载次数为 0，且候选检查量不随总记录数线性增长；浏览器验证键盘回填不引发 transcript 滚动。

2026-09-09 验收证据：24 项前端测试通过，包括真实 composer 的斜杠菜单优先与原提交回调；4 项 Rust 输入历史测试通过，覆盖分页、身份去重、冻结 head、原文过滤、重开及长原文。Windows release 一万条记录、索引命中时，50 条摘要页 120ms、单条正文 135ms，摘要响应小于 16 KiB；此为本机单次测量，不是并发写入或冷索引 SLA。debug 初始版本为约 600ms/492ms，不用它代表发布性能。浏览器复用实际 AcpConversationComposer，确认循环、清空、斜杠回填不弹菜单且可发送、多行边界及 transcript scrollTop 不变。生产构建通过，保留项目原有大 chunk 提示；cargo check 通过，存在原有 unused 警告。

边界：来源不明且无标记的旧消息不参与回填；已由不可变 RawAgent 快照确认的错误 false 标记通过一次性迁移修复。浏览器 demo 默认无持久化输入历史。真实桌面 IPC 由编译与 Rust 接口测试验证，浏览器交互使用同一 composer 及受控接口数据验证。

## 2026-09-09 测试反馈修复

现场证据：本机 task-007 有 5 条持久化根用户输入，早期索引仅允许最近 3 条。首条 originalUserText=false；第二条比首条晚约 55 分钟，originalUserText=true，但带 manual-follow-up 控制权转换元数据。原始内容没有按时间删除。工具栏查询时三个配置按钮 x 坐标均增加 20px，完成后恢复。

根因属于正确设计下的数据来源投影和布局实现不完整：首轮 RawAgent requirement 未进入 display_text；筛选将控制权转换误认为控制提示词；临时 spinner 参与 flex 排版改变可用宽度。修复复用 PromptBundle、Timeline、现有 schema 迁移和 prompt-kit，不新增历史存储或无界缓存。

失败测试：Direct 首轮原文断言得到 None；手动追问分页得到 0 条而非 1 条；旧首条来源迁移未执行；工具栏 loading 阶段多出一个 SVG 布局子元素。均先在未修复实现上观察失败。

实施：RawAgent + New + RequirementTask 显式填充原文；资格判断根据原文和可见性，允许用户消息携带 runtimeControl；Timeline 索引版本升为 12；复用 attempt schema 升为 3 做严格快照验证的一次性首条来源修正，锁内追加 metadata patch 后再推进 schema，崩溃重试幂等；按用户最新要求移除历史查询 spinner 及占位，工具栏结构不随查询状态变化。

性能与过度设计审视：缓存仍为 40 条摘要和 3 条/约 1 MiB 正文，没有时间 TTL。普通按键查询复杂度与上一版相同，无新增 I/O。迁移只在旧 attempt 首次准备时读取 node、不可变 workflow snapshot、现有索引和最多一条正文，满足条件时追加一条 patch；不扫描目录或读取全部原文。现有索引需要升级时仍复用既有重建机制，不把一次性成本描述成常数时间。

- [x] 原文筛选与布局失败测试转绿；浏览器正常/窄窗/重新拉宽的加载前中后坐标一致。
- [x] 首条迁移 5 类正反例、小时级时间推进与缓存重建测试通过；前端 4 个文件共 26 项测试通过，后端共享存储与运行时回归 76 项通过、1 项依赖外部 fixture 的既有测试默认跳过。前端生产构建和桌面 cargo check 通过，保留既有 chunk 与 unused 警告。
- [x] 问题会话隔离副本验证：全部 5 条可达，首条原文与迁移前一致，第二次迁移不再写入；未直接修改用户会话数据。

验收后已关闭验证浏览器页和自建 Vite 服务，删除仓库内临时验证页面与示例。系统临时目录中的隔离会话副本删除被自动安全策略拦截，副本保留在临时目录，不进入工作区。

用户随后要求取消输入历史旋转图标：移除图标、空白占位和中英文加载文案；保留既有查询、重复按键控制及错误反馈。复用 prompt-kit composer，无新增组件、依赖、状态或 I/O，减少工具栏 DOM，不引入性能风险。18 项相关前端测试通过；内置浏览器验证宽窗、窄窗及重新拉宽时无加载图标、原文可回填、加载前后按钮坐标一致。

## PR #120 格式验收修正

GitHub verify 在 `Check formatting` 失败，后续 Rust 测试、前端测试和构建被跳过；本地同一 `cargo fmt --all -- --check` 命令已复现退出码 1。根因为提交前验收遗漏全量格式检查，包含本次 branches/provider 改动以及既有代码的排版差异，不是功能设计缺陷。使用官方 `cargo fmt --all` 统一修正，原失败命令转绿；不修改 CI 门禁。PR 标题应按 Conventional Commits 使用 `feat(acp): add input history navigation and pin raw frame controls`，由用户自行更新与提交。

验证：`cargo fmt --all -- --check` 和 `git diff --check` 通过；`cargo test -p gold-band --lib -- acp::timeline::composer_history::tests render_prompt_bundle_does_not_add_builtin_output_contracts` 共 7 项通过。格式修正与本节记录作为独立提交推送，远程 CI 需在推送后重新验证；PR 标题由用户自行更新。

性能与过度设计审视：本次仅格式化和交付文档修正，不新增依赖、状态、缓存、I/O 或运行期逻辑；使用既有格式检查作为回归契约，无需新增重复断言。

## 2026-09-10 合并门禁与交互性能修正

合并最新 main 后，acceptance profile 契约测试稳定失败，因为中英文提示词只写了“不得仅据此声明 BLOCKED”，没有明确表达“外部证据缺失不构成阻塞条件”。这是正确的统一提示词设计下，中英文文案与测试契约未完整对齐；同步补齐两种语言，不改验收状态机或测试标准。

输入历史延迟的根因是读取层复用不完整：每次分页重新反序列化完整 Timeline 物化索引、扫描所有 locator 并排序，正文读取也重复加载完整索引。修复扩展现有 item reader 有界 LRU 投影，加入可重建的有序 Composer 摘要与 prompt identity 去重位置，并让 tail replay 同步更新；Timeline 继续是唯一事实源，没有新增持久 schema、缓存服务、依赖或跨层 identity。

先增加一万条记录回归断言，确认旧实现预热后仍发生 2 次完整索引加载，再修改实现。修复后 `cargo test --lib composer_history` 6 项、`cargo test --lib item_reader` 3 项及 acceptance profile 定向测试通过。Windows release 一万条记录热路径单次测量为 50 条摘要页约 224 微秒、单条正文约 220 微秒，摘要响应继续小于 16 KiB；回归同时断言预热后的完整索引加载次数为 0，候选检查量不超过页大小同阶。冷索引加载、重建和并发写入不属于该热路径测量。

性能与过度设计审视：热分页由每次 O(N log N) 降为有序范围查询加页内去重检查，正文复用 locator 做单条读取；tail append 以 O(log N) 更新二级索引。读取投影仍受既有 4 项、32 MiB LRU 双重约束，淘汰后可由 canonical Timeline/index 重建，不形成第二事实源或无界缓存。新增二级索引直接对应 10,000 条交互延迟和分页不变量，复用现有生命周期与锁边界，没有为假设性需求引入队列、并发机制或新抽象层。

## 2026-09-11 完整草稿与取消恢复

根因：原历史交互只允许空输入且把历史文字直接写入草稿，无法表达临时翻阅与完整草稿之间的往返；提交异常分支又把取消请求误当成接收成功，提前释放已分离草稿。前者是草稿投影契约缺失，后者是正确提交生命周期下的判断错误。

复用既有 `AcpComposerDraft` store、prompt-kit 和提交链路：普通草稿与取消后恢复的草稿统一保存文字、引用、文件、图片；历史只投影文字，Down 越过最新或 Escape 返回完整草稿。编辑或发送历史时采用新的纯文字草稿，释放被替换的预览资源；附件解析显式消费本次提交快照，不读取旧 render 捕获的附件集合。附件选择和引用新增仍作用于原草稿，实际上下文变化结束翻阅；取消文件选择及拖拽悬停不丢弃原草稿。

接收判定只使用明确的提交响应或匹配 prompt identity 的 canonical admission。取消请求本身不再标记接收成功；未接收提交失败恢复完整快照，不覆盖其间的新输入。已接收消息停止生成不回填。输入法开始组合前采用可见历史文字；外部正文变更、会话切换及首次查询中向下均使迟到读取失效。

验证证据：修改实现前，新增草稿往返测试在 ArrowUp 入口失败；取消中的提交失败测试期望恢复正文但得到空字符串；首次查询期间向下的测试被迟到正文覆盖。分别确认根因并使用相同测试转绿。11 个相关前端测试文件共 143 项通过，覆盖真实 ACPChatDialog 的图片、文件、引用恢复，停止响应先后顺序，历史普通发送与继续并发送不夹带隐藏上下文，accepted 响应无 timeline admission 时不重复回填，以及 IME、分页、草稿作用域和消息渲染隔离。

浏览器在独立测试夹具中挂载真实 ACPChatDialog，使用受控 RuntimeApi 响应；验证完整草稿上下键往返、发送中停止后的恢复、历史发送仅包含 `displayText` 与空 quotes、图片解码成功。1440px、390px 及重新拉宽验证通过，窄窗 scrollWidth 等于 viewport，输入框与操作按钮无重叠。此验证不等同于真实 EXE/provider 端到端验证。前端类型检查和生产构建通过，保留既有大 chunk 与静态/动态导入提示。

性能与过度设计审视：新增状态仅为 composer 局部历史文字投影，原草稿仍由既有最多 64 项、附件预算 100 MiB 的 store 管理；不复制 File 或图片字节，不新增依赖、持久字段、全局 Context、历史扫描或 IPC。翻阅继续使用既有有界历史 reader；提交附件解析最多处理既有 10 个附件，仅在发送时执行。复用原接收和恢复接口，无额外消息模型或队列，渲染隔离回归通过。

## 2026-09-11 提交交付事实与停止回填

根因：发送路径在草稿分离后只看命令返回的 `accepted` kind 就释放草稿快照，把“后端已接纳”当成“消息已进入 transcript”。ACP 提示在命令返回前已写入 turn，但对应 canonical `goldBandPrompt` 事件可能还在路上；若用户在这段“发送中”窗口点击停止，或 turn 在 admission 前因会话配置等错误进入 failed 终态，前端已经清掉 optimistic 气泡并释放快照，消息既不在 transcript 也不在 composer，用户输入彻底消失。这是正确的“canonical admission 才算交付”设计下，消费端交付判定过早。

修复复用既有 `AcpComposerDraft` store、optimistic promptId、`findMatchingGoldBandUserPrompt` 与提交链路，新增一个按 promptId 索引的待交付快照投影：分离草稿时登记完整 draft（正文、引用、文件、图片），匹配 promptId 的 canonical admission 到达后释放附件预览资源；停止请求成功、或 turn 在 failed / cancelled 终态且无 admission 时，通过同一草稿恢复接口回填并移除对应 optimistic 气泡；turn 完成为已消费，只释放快照。提交失败仍在同一 finally 分支恢复，覆盖提交前配置校验阻止 prompt 的情况。快照不覆盖用户其间的新输入，组件卸载释放未结算预览资源。

先补最小失败测试：发送中点击停止后 `textarea` 仍为空；accepted 响应后 turn 在无 admission 时 failed，`textarea` 仍为空。两条都与根因分析一致，分别在实现前稳定失败。修复后 25 项会话提交测试中 24 项通过，唯一失败 `uses a stale run error only as fallback for ACP diagnostics` 经 `git stash` 对照确认在修复前的主工作区基线同样失败，与本次改动无关。新增覆盖：停止后恢复完整图片、文件与引用且不撤销预览 URL；accepted 但无 admission 时 failure 回填、completed 不回填；admission 到达后才释放附件。

验证：`web/tests/acp-runtime-continue-submit.test.tsx` 24/25（1 项基线失败）、相关 composer 回归与 `tsc -p web/tsconfig.build.json`、前端生产构建通过。浏览器在真实 `ACPChatDialog` 夹具中验证发送中停止后草稿与两张附件、引用回到 composer，输入框恢复可编辑。

性能与过度设计审视：新增结构是一个只覆盖在途提交的小 Map，条目在 admission、终态或停止任一结算点立即删除，不随历史增长；每个条目复用既有草稿对象，不复制附件字节，也不新增 IPC、持久字段、定时器或全局 Context。admission 检测复用既有 `mergeAcpEvents + findMatchingGoldBandUserPrompt`，仅在存在在途快照的短窗口内按 promptId 精确匹配，不扫描历史、不轮询。
