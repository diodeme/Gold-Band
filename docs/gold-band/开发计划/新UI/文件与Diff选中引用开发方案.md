# 文件与 Diff 选中引用开发方案

日期：2026-09-28

产品设计见 [会话式运行时 · Composer 上下文功能区与引用](../../产品设计文档/interaction/app/conversational-runtime.md) 与 [定时任务管理](../../产品设计文档/interaction/app/scheduled-task-management.md)。

## 目标

- 右侧工作区打开的文件（工作空间、运行目录、会话附件/产物、草稿附件、本轮文件版本）选中文本后可引用到 composer。
- 文件 diff（Agent 本轮修改、Git 暂存/未暂存、提交、PR）支持选中引用与头部“引用完整 diff”；工作空间文件头部提供“引用到对话”。
- 引用在添加时冻结；定时任务可携带引用。

## 数据

- `UserPromptQuote { id, text, source }`，`source` 为 `kind` 标签联合：`agentMessage { messageKey }`、`file { label, startLine, endLine }`、`diff { path, origin, revision, scope }`。替换旧的 `sourceMessageKey`；读取时（Rust serde `from` 与前端 timeline 解析）把只有 `sourceMessageKey`、没有 `source` 的旧引用视为 Agent 消息引用，写入统一使用新结构。
- 预算：非整文件 diff 引用合计 12,000 字符（Unicode 标量）；整文件 diff 合计 64,000 字节；最多 64 条。前后端常量分别位于 `src/provider/quotes.rs` 与 `web/src/lib/composer-context.ts`。
- 错误码：`conversation.prompt-quote-{count-exceeded,invalid,metadata-too-long,limit-exceeded,diff-limit-exceeded}`，后端只返回 code + params；定时任务编辑新增 `scheduled.quote-not-stored`。

## 实现

- [x] 后端 `src/provider/quotes.rs`：类型、校验、按语言渲染 `runtime/user_quote.md`（7 种语言）；timeline 中整文件 diff 正文置空。
- [x] 新会话的引用、工作空间文件与角色作为任务输入写入 `authoring/task-prompt-input.json`；定时任务内容快照 `quotes` 参与指纹（不含 ID），编辑只允许按 ID 删除。
- [x] 前端 `diff-quote.ts`：基于 `@codemirror/merge` 行对齐 chunk 生成 git 风格 hunk；选区扩展到触及的整个 chunk，选区读取以编辑器状态选区为准，删除块由选区触及的 chunk 判定。
- [x] 修复：从新增/上下文行向上拖入删除块时选区被钳在块边界、红色无选中效果，起止点重合时选区消失。根因：删除块是无文档位置的 block widget，从文档行开始的拖动由 CodeMirror 鼠标选区接管，坐标只能换算到 widget 边界并回写 DOM 选区；另外选区覆盖整块但端点不在块内时，引用读取漏掉该块。修复：`ReadonlyUnifiedDiff` 通过官方 `EditorView.mouseSelectionStyle` 在指针位于删除块时把选区头放到块的另一侧，整块选中（单击拖动生效，双击/三击沿用默认）；`readUnifiedDiffSelection` 计入选区相交的所有删除块。先补 `readonly-unified-diff.test.tsx` 与 `editor-selection-quote.test.ts`「runs across」用例，修复前 3 条失败，修复后通过；浏览器验证上下两个方向拖动、从删除块起拖、双击选词与引用正文。
- [x] 修复：从删除块内发起选中不稳定，拖到一半选区消失或跳到别处。根因：删除块 widget 默认忽略事件，按下后由浏览器原生选区接管；CodeMirror 读选区时跳过锚点在被忽略 widget 内的原生选区，状态选区保持旧值，只读未聚焦视图在滚动重绘时（`selectionNotFocus`）把旧状态选区写回 DOM，原生选区被覆盖。修复：选区统一由编辑器接管，删除块 `pointer-events: none`，`diff-chunk-selection.ts` 以 `elementAtHeight` 识别指针下的删除块；未变更行自由选中，选区触及变更时吸附整个 chunk（删除块起点到新增行末尾），按下删除块、拖入删除块、双击/三击删除块都整块选中；`transactionFilter` 对其他指针选区（如双击新增行单词）同样吸附；复制含变更的选区时写入与引用相同的 diff 片段（只读视图无焦点，copy 事件落在 scroller，监听挂在编辑器根节点）；`readUnifiedDiffSelection` 改为从状态选区计算，DOM 选区只提供按钮位置。先改 `readonly-unified-diff.test.tsx`、`editor-selection-quote.test.ts`，修复前失败、修复后通过；浏览器验证从删除块按下拖到边缘自动滚动后选区保持、跨出 chunk 后选区头自由、上下文拖入吸附、双击、复制输出。
- [x] `editor-selection-quote.ts` 读取 DOM Selection（只读视图无焦点）；`EditorSelectionQuote` / `DiffSelectionQuote` 复用通用 `SelectionQuoteButton`，经 `ComposerReferenceTarget.addQuote` 送入当前 composer。
- [x] composer chip 按来源显示图标与标签，超预算提示给出大小、上限与剩余量；用户消息引用弹层展示来源；定时任务编辑面板展示可删除的引用标签。

## 验收

- Rust：`quotes` 模块测试（三类渲染、7 语言、独立预算、无效来源）、指纹、定时任务冻结与只删、timeline 置空。
- Web：`diff-quote`、`editor-selection-quote`（jsdom 下真实 merge view 的删除块选择）、`composer-context`、`composer-quote-i18n`、`user-message-quotes`、`conversation-composer-draft`、`workspace-file-reference-bridge`。
- 浏览器：文件多行/单行选中引用、文件头引用、diff 仅选删除行扩展为整块、整文件 diff 引用、重复引用提示、发送后引用弹层、定时任务创建携带引用与编辑删除。

## 评审

- 过度设计：复用既有 composer 草稿、bridge 与 prompt 模板体系，仅新增来源联合与一个 diff 格式化函数，没有新增持久化实体或状态机。
- 性能：选区读取只在 mouseup/shift-keyup 触发；diff 格式化 O(行数)，仅在用户点击时运行；整文件 diff 不写入 timeline 正文，避免事件文件膨胀。

## 任务输入随节点下发（2026-09-28）

- [x] 根因：引用、工作空间文件与角色被建模为“首条消息的展示输入”（`prompt_display`），只有入口节点首个 prompt 携带；后继节点、新 round、AI-DYNAMIC 叶子节点看不到，手动重试非入口节点却会拿到，行为不一致。
- [x] 方案：新增 `TaskPromptInput { quotes, role, workspaceFiles }`，与 requirement、`authoring/inputs` 同属任务输入，替换原先三个 `initial-prompt-*.json`。`build_worker_invocation` 与 `build_dynamic_worker_invocation` 在新会话时与任务附件一起加载到 `WorkerInvocation.task_prompt_input`；`render_prompt_bundle` 在 `RequirementTask` 渲染时用它包装 requirement、解析工作空间文件并写入 prompt 事件元数据，其余模式仍使用本轮用户消息的 `prompt_display`。Continue 会话与 artifact finalize 不重复发送。
- [x] 角色只在 Direct 生效：新建校验对非 Direct 返回 `role.direct-only`，非 Direct 运行的追问返回 `conversation.prompt-role-direct-only`（7 种语言文案）；首页 composer 在 Workflow / AUTO 下 `@` 不列角色，运行页 `ACPChatDialog` 通过 `roleSelectionEnabled` 仅在 Direct 加载角色。AUTO 与 Workflow 同样处理。
- [x] 新建会话改为完整校验 prompt 输入（引用、角色长度等），不再静默丢弃不完整角色。
- [x] 验收：`tests/provider_prompt_bundle.rs` 新增三项（每个新 requirement 渲染携带引用与工作空间文件、角色包装、Continue 不重发）；`node_executor` New/Continue 构建断言；`dynamic_leaf_new_session_carries_the_task_quotes`；`a_role_is_accepted_only_for_direct_conversations`。浏览器验证首页 AUTO / 工作流模式输入 `@` 无角色菜单、Direct 正常列出角色；运行页 composer 因 mock 无法进入工作流运行页未做浏览器验证。
- 过度设计：复用任务附件的加载时机与 `WorkerInvocation` 管道，只新增一个结构与一个字段，合并三个文件为一个，未新增状态或生命周期。
- 性能：每个新会话多读一个小 JSON（与读取 inputs 目录同量级），workspace roots 仅在确有工作空间文件时加载；Continue 与 finalize 无额外开销。开发阶段不迁移旧的 `initial-prompt-*.json`，此前创建且尚未结束的任务后续节点不再带这些输入。

## `@` 分类菜单：文件与角色（2026-09-28）

- [x] 交互：`@`（仅开头触发）先列分类，Direct 为文件、角色，Workflow / AUTO 只有文件；回车进入分类，文件按目录层级浏览，Backspace / ← 返回上一级；`@` 后输入内容跨分类检索。选中文件加入工作空间文件引用并清除 `@查询`；文件夹不可引用。
- [x] 修复：已有正文时在开头输入 `/` 或 `@` 不打开菜单。根因是菜单查询与选中替换都按整段输入计算（`^/...$`、整段覆盖），光标后的正文让匹配失败。改为按光标计算：`matchComposerMenuQueryAt(input, caret)` 只匹配开头到光标，`replaceComposerMenuQuery` 只替换这段并保留其后正文；控制器通过 `textareaRef` 跟踪光标并在替换后恢复。先补 `composer-mention-menu.test.tsx`「menu trigger typed before existing text」与 `slash-command.test.ts` 纯函数用例，修复前 3 条失败，修复后全部通过。
- [x] `@` 分类行名称前加图标（文件 `FileText`、角色 `Brain`），映射放在 `SlashCommandMenu` 的 `MENTION_CATEGORY_ICONS`；角色、命令、文件行不变。`composer-mention-menu.test.tsx`「@ mention menu rows」验收。
- [x] 实现：`slash-command.ts` 新增 `MentionView` 与纯函数 `buildMentionGroups` / `mentionFilesRequest` / `parentMentionView`，分组支持 `status`（loading / empty / error）原位展示；`useMentionWorkspaceFiles` 复用 `listWorkspaceDirectory` 与 `searchWorkspaceFiles`，搜索 150ms 防抖、最多 20 条，以 generation 丢弃迟到响应；`useSlashCommandController` 增加可选 `mention` 配置承载导航与键盘处理；两个 composer 把原 bridge 内的文件引用逻辑提取为共用 `addWorkspaceFile`，右侧文件树与 `@` 选择走同一路径。菜单为分类与目录行加箭头，文件行显示所在目录；7 种语言新增菜单文案。
- [x] 验收：`web/tests/composer-mention-menu.test.tsx`（分类、目录浏览与状态、跨分类检索、请求映射、键盘进入/返回/选中、加载中回车不发送、防抖与迟到响应丢弃）。浏览器验证首页 Direct 分类、目录浏览、选中文件变为引用标签、跨分类检索与无结果状态、Workflow 只有文件分类、Backspace 返回；运行页 composer 共用同一控制器，mock 无法进入运行页，未单独浏览器验证。
- 过度设计：无新后端接口与持久化，导航状态只存在于菜单打开期间；复用现有菜单组件与文件引用链路。
- 性能：目录只在进入时读一层；搜索防抖、限条数、只保留最新结果；菜单关闭后不发请求；输入时只重算菜单分组，不影响历史消息渲染。
