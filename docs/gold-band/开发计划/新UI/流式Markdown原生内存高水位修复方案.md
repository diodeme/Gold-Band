# 流式 Markdown 原生内存高水位修复方案

## 目标

保留 ACP 流式消息的逐字 Markdown 体验，同时消除 Gold Band 每 32 ms 截取累计前缀、触发 Streamdown 全文 incomplete repair 的热路径。Markdown 结构只由 Streamdown 管理，不新增闭合检测或自行分块。

## 实施项

### 根因修复

- [x] 删除 `StreamingMarkdownPresentation` 的 32 ms canonical prefix RAF；Markdown 只消费 latest-wins 发布的 canonical snapshot。
- [x] 使用 Streamdown 2.5 正式 `animated` rehype 扩展点生成 renderer 字符 token；由文档级唯一水位按 DOM 顺序播放，替代每个 block 独立从零计时的默认动画。
- [x] 播放积压按 42～180 字符/秒动态追赶；唯一 RAF 只切换 renderer token 状态，不提交 React state、不截取 canonical、不触发 Markdown 解析。
- [x] 字符显现去掉 blur/filter，只保留 opacity；扩展默认关闭的 ACP streaming 诊断，记录 reconcile 耗时、约 500ms 播放摘要、超过 50ms 的长帧和 settle 原因，不记录正文或逐字符事件。
- [x] 播放器按 Streamdown block DOM 身份缓存 token 与文档水位；MutationObserver 只重建变化 block，稳定 block 不再重复 query/扫描/写状态，追加更新成本从累计全文 token 收敛到变化 block token 加 block 索引维护。
- [x] 积压归零时重置 RAF 帧时钟与诊断采样窗口，避免后续 token 到达时把无播放工作的空闲时间误报为长帧。
- [x] 扩展默认关闭的性能取证：播放摘要记录 tick 总耗时/最大耗时；浏览器 Long Animation Frame 记录 blocking/render/style-layout 时长与有界 script attribution；会话 ResizeObserver 和自动贴底按 500ms 汇总次数、尺寸变化、滚动写入和回调耗时。诊断不记录正文或脚本 URL。
- [x] 保持单个 Streamdown 文档上下文，incomplete Markdown、代码围栏、链接、HTML、表格、脚注和引用都由 Streamdown/remend 解释；Gold Band 不实现语法判断。
- [x] 新增 append-only block parser cache，只重新 lex 上一个可变尾块，稳定历史块保持不变。
- [x] 流结束时立即切到 static Streamdown，移除动画 metadata 并保持完整 canonical，不等待播放 backlog。
- [x] 消息 React key 加入会话 event-window identity，provider 在不同会话复用 event id 时不复用 parser/动画状态。

### 回归测试

- [x] 固化 Markdown 组件最多创建一个可取消 RAF，且 RAF 只释放 Streamdown renderer token，不调用 Markdown parser 或 React render。
- [x] 固化列表 marker、列表正文、后续标题和段落共享严格文档顺序，不允许跨 block 并行动画。
- [x] 固化积压速度上下限、append-only 尾块重建后的水位迁移，以及非追加改写立即 settle 的安全基线。
- [x] 固化 canonical 全文始终在 DOM，旧消息 settle 后动画 metadata 立即移除，新消息独立动画。
- [x] 固化单个 Streamdown 文档上下文，跨块脚注/引用语义不因性能优化拆散。
- [x] 固化 append-only 只重算可变尾块、历史改写回退全量解析。
- [x] 固化 append-only 尾块替换时至少复用一个稳定 block，且 `scannedUnitCount` 小于全文 `unitCount`；播放水位与严格顺序保持不变。
- [x] 固化 Long Animation Frame 摘要最多保留三个 script attribution，且播放 tick、Resize 和贴底诊断均只记录有界数值摘要，不包含正文。
- [x] 保留不完整 Markdown、链接代理及代码 DOM 契约测试。
- [x] 固化跨会话相同 event id 使用不同 render key。
- [x] 保留“打开 15 个文件时历史 Markdown 不重渲染”测试。
- [ ] 使用 `npm run dev` 复测 6 KB 以上正文、未闭合代码围栏、流结束和紧接下一消息，确认动画连续且旧消息立即完整。
- [ ] 使用 `npm run dev:static` 开启 `goldBand.debug.acpStreaming` 复测并导出日志，确认剩余卡顿来自 Markdown render、DOM reconcile、播放 RAF 或滚动/layout 中的哪一层。

### 真实开发版验收

- [x] 修复前采集 baseline / target / final 与 memlab retained chain。
- [x] 开发版重启后基线恢复为 V8 27.24 MB、embedder 9.76 MB、4,168 DOM nodes，CDP 入口持续可用。
- [ ] 确认长流式输出期间每帧只解析当前可变尾块，不重复解析稳定历史块。
- [ ] 确认工作区标签关闭后不存在 CodeMirror、累计字符串或大型 detached DOM retained chain。

## 验收标准

接口层以库职责、完成收敛和解析范围为硬约束：Gold Band 不解析 Markdown 闭合结构；Streamdown streaming mode 负责 renderer token、incomplete repair 与 block 语义，append-only 更新只能把其可变尾块交给 block lexer；文档播放层至多一个 RAF，并且只释放严格连续的 DOM token 前缀。稳定 Streamdown block 的 token 索引必须复用，DOM 更新的扫描与状态校准只覆盖变化 block；流结束与会话切换立即以 canonical 静态完整展示。现场层要求 `npm run dev` 的长正文不再因 32 ms 全文前缀循环或播放层全文 token 重扫出现秒级跳字，也不得出现列表与后续标题并行播放；生产构建仍用于最终性能基线，但不能掩盖开发构建的数量级卡顿。
