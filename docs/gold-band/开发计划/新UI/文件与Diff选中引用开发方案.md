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
- [x] 新会话初始输入写入 `authoring/initial-prompt-quotes.json`；定时任务内容快照 `quotes` 参与指纹（不含 ID），编辑只允许按 ID 删除。
- [x] 前端 `diff-quote.ts`：基于 `@codemirror/merge` 行对齐 chunk 生成 git 风格 hunk；选区扩展到触及的整个 chunk，删除块由选区端点所在 `.cm-deletedChunk` 判定。
- [x] `editor-selection-quote.ts` 读取 DOM Selection（只读视图无焦点）；`EditorSelectionQuote` / `DiffSelectionQuote` 复用通用 `SelectionQuoteButton`，经 `ComposerReferenceTarget.addQuote` 送入当前 composer。
- [x] composer chip 按来源显示图标与标签，超预算提示给出大小、上限与剩余量；用户消息引用弹层展示来源；定时任务编辑面板展示可删除的引用标签。

## 验收

- Rust：`quotes` 模块测试（三类渲染、7 语言、独立预算、无效来源）、指纹、定时任务冻结与只删、timeline 置空。
- Web：`diff-quote`、`editor-selection-quote`（jsdom 下真实 merge view 的删除块选择）、`composer-context`、`composer-quote-i18n`、`user-message-quotes`、`conversation-composer-draft`、`workspace-file-reference-bridge`。
- 浏览器：文件多行/单行选中引用、文件头引用、diff 仅选删除行扩展为整块、整文件 diff 引用、重复引用提示、发送后引用弹层、定时任务创建携带引用与编辑删除。

## 评审

- 过度设计：复用既有 composer 草稿、bridge 与 prompt 模板体系，仅新增来源联合与一个 diff 格式化函数，没有新增持久化实体或状态机。
- 性能：选区读取只在 mouseup/shift-keyup 触发；diff 格式化 O(行数)，仅在用户点击时运行；整文件 diff 不写入 timeline 正文，避免事件文件膨胀。
