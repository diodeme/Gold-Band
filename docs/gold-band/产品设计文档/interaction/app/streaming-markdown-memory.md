# 流式 Markdown 渲染与原生内存约束

## 问题定义

ACP 会话以累计文本快照更新正在生成的消息。历史消息、当前流式消息和右侧工作区属于不同生命周期：历史消息应保持静态，当前消息只应响应新的累计快照，工作区标签变化不应驱动任一消息重新解析。

此前前端在 ACP 125 ms latest-wins 发布之外，又为每条流式 Markdown 建立 32 ms 的逐字展示循环。每个 animation frame 都截取更长的累计前缀并重新调用 Streamdown，导致同一长消息持续执行全量 Markdown 解析、React reconciliation、DOM 创建和 layout。对象最终可被 GC 并不代表没有性能问题：Chromium 的原生分配器会保留高水位页，Renderer 工作集因此持续升高并造成系统内存压力。

## 现场证据

真实开发版会话的 baseline / target / final 指标如下：

| 指标 | baseline | target | 关闭 5 个工作区标签后 |
| --- | ---: | ---: | ---: |
| V8 used heap | 27.49 MB | 49.36 MB | 71.23 MB |
| V8 total heap | 28.72 MB | 120.05 MB | 122.10 MB |
| Blink embedder heap | 7.46 MB | 50.73 MB | 84.74 MB |
| DOM nodes | 4,168 | 6,912 | 6,201 |
| layout objects | 1,710 | 4,243 | 3,645 |
| Renderer private bytes | 约 158 MB | 约 823–868 MB | 仍处于高水位 |

memlab 在 target 到 final 间识别到 28 个 detached React/DOM 泄漏簇，主要是 Fiber、头像图片 effect、Markdown/ACP 小节点，合计 retained size 不足约 1 MB；没有发现能解释数百 MB 的 CodeMirror 或累计字符串保留链。因此主要问题是高频全量重解析产生的原生分配 churn，而不是单个大型 JS 对象永久持有。

## 根因设计

产品必须同时保留逐字节奏和流式 Markdown，且 Markdown 语法边界只能由 Streamdown 管理，Gold Band 不得实现第二套闭合检测或分块规则。渲染链路拆成三层：

1. ACP live event buffer 按流 identity 只保存最新累计快照，并以 125 ms 为最短发布间隔。
2. Markdown 组件把最新 canonical snapshot 直接交给单个 Streamdown 文档实例；Streamdown 2.5 的正式 `animated` rehype 扩展点只负责把已解析 HAST 文本暴露为字符 token，不再使用其每个 block 独立从零计时的默认播放语义。Gold Band 在渲染后 token 层维护一条文档级顺序水位和至多一个 RAF，按 DOM 文档顺序释放 token；积压越大，速度在 42～180 字符/秒之间单调提升。播放索引以 Streamdown block DOM 身份缓存每个 block 的 token 列表与文档起止水位；DOM 更新只重建被替换或内部变化的 block，稳定 block 复用索引且不重复查询、扫描或写 token 状态。该 RAF 只沿 block cursor 切换已有 DOM token 的展示状态，不截取 canonical、不提交 React state，也不触发 Streamdown 解析。
3. Streamdown 负责 incomplete Markdown repair、block 语义和 AST。`createIncrementalMarkdownBlockParser` 只作为 Streamdown 官方 `parseMarkdownIntoBlocksFn` 扩展点缓存其返回的已完成块，追加内容时只把上一个可变尾块送回同一个 Streamdown block lexer。Gold Band 不判断代码围栏、链接、HTML、表格或强调是否闭合，也不把 blocks 拆成多个 Streamdown 实例，避免破坏脚注与引用等文档级语义。
4. 非追加改写才回退为一次全量 block parse；这是正确性兜底，不是正常流式路径。
5. 已完成消息通过稳定字符串 props 与 `React.memo` 保持引用稳定；工作区 tabs、width、activeTab 变化不得穿透命令 context 触发历史消息更新。

2026-08-04 的首轮优化只约束了 Streamdown block lexer 和 Block DOM reconciliation；Streamdown 在调用自定义 parser 前仍会对 Gold Band 每帧提供的完整可见前缀执行 incomplete repair，本地链接代理和前缀规范化也会扫描整段文本。因此“block lexer 已增量化”不能证明“整条链路已增量化”。新设计删除 Gold Band 的高频前缀推进，让 Markdown 只随 125 ms canonical 发布更新；已完成 block 继续由 Streamdown memo 稳定，逐字调度只操作 renderer 已生成的 DOM token。

## 生命周期与接口不变量

- 数据层：ACP cumulative snapshot 是唯一持久 canonical text；Markdown 组件不维护第二份 Markdown 可见前缀。播放层只维护 renderer token 的瞬时文档水位，不能反向成为正文事实源。
- 调度层：每条流式消息至多一个可取消 RAF，只按文档顺序切换已有 token 的 pending/revealed 状态；不得在 RAF 中提交 React state、截取 Markdown、调用 parser、查询整篇 token DOM 或扫描 canonical 全文。Streamdown 因累计快照替换尾块时，只校准发生变化的 block，并以文档 token 顺序迁移已播放水位；稳定 block 的索引与状态必须复用。积压归零时重置帧时钟和采样窗口，后续新增 token 不得把无播放工作的空闲时间误报为 RAF 长帧。
- 解析层：append-only 更新只重算 Streamdown 返回的上一可变尾块；稳定块不得再次进入 block lexer。Markdown 结构与未闭合语法仅由 Streamdown/remend 解释。
- 展示层：DOM 始终包含当前 canonical 的完整安全 Markdown 结果；列表 marker、正文、后续标题和段落共用一条文档级播放顺序，不得由各 block 并行动画。不得用 Gold Band 自制解析器拆分文档上下文。
- 收敛层：`streaming=false` 时取消唯一 RAF、释放所有 pending token，并移除动画插件和 incomplete repair，立即以同一 canonical 静态渲染；不得等待播放 backlog。
- 身份层：消息 React key 必须包含会话 event-window identity 与事件 identity；切换会话时即使 provider 复用 event id，也不得复用上一会话的动画或 parser 状态。
- 完成层：静态 Markdown 不订阅右侧工作区展示状态，只通过稳定 `openResource` 命令处理链接。
- 清理层：组件卸载或任务替换时取消唯一 RAF、断开 DOM observer，不保留累计前缀版本或旧节点队列。

## 回归验收

- streaming 时字符 token 必须来自 Streamdown 正式动画扩展点；文档调度器最多保留一个 RAF，且每次只能释放严格连续的文档前缀，不能出现列表正文与后续标题并行推进。
- 字符显现只使用 opacity，不使用 blur/filter；诊断开启时按初始化、DOM reconcile、约 500ms 播放摘要、超过 50ms 的播放帧、浏览器 Long Animation Frame、会话内容 ResizeObserver、自动贴底和 settle 采样。Long Animation Frame 只保存 blocking/render/style-layout 时长和最重的三类 script attribution，不保存脚本 URL；Resize/贴底只保存 500ms 窗口内的次数、尺寸变化、滚动写入和回调耗时。禁止按字符记录、保存正文或在热路径跨 IPC 落盘。
- canonical 全文始终存在于 DOM；流结束、工具边界、用户新 prompt 或会话切换后必须立即移除旧消息动画 metadata 并保持完整文本，不能重新进入会话才补齐。
- 未闭合 Markdown 始终由单个 Streamdown/remend 文档上下文修复；代码围栏、链接、HTML 和表格不得由 Gold Band 自行解析。
- append-only 长消息的 block lexer 输入不得重复包含已经稳定的历史块；非追加改写必须正确回退全量解析。
- 跨块脚注/引用等文档级语义必须保持，不能为了冻结 blocks 建立多个 Streamdown 根实例。
- append-only 尾块被 Streamdown 重建时，已播放文档水位不得回退或重播；非追加改写立即完整 settle 为新基线，后续追加从该基线继续。
- append-only 更新中未被 Streamdown 替换的 block 必须复用 token 索引；诊断中的 `reusedBlockCount / rebuiltBlockCount / scannedUnitCount` 应能证明扫描量受限于变化 block，而不是累计全文 token 数。
- 不同会话复用相同 provider event id 时，React render key 必须不同。
- 打开 15 个文件时历史消息不重渲染。
- 6021 帧回放仍只保留每个 identity 的最新累计文本。
- 真实开发版复测必须记录 baseline / target / final 的 V8、embedder、DOM、Renderer private bytes，并用 memlab 确认 detached DOM、CodeMirror 和累计字符串不存在大 retained chain。
