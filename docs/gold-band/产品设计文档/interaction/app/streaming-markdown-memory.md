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

产品必须同时保留逐字节奏和流式 Markdown，不能用整块淡入或流结束后再格式化替代。渲染链路拆成三层：

1. ACP live event buffer 按流 identity 只保存最新累计快照，并以 125 ms 为最短发布间隔。
2. `StreamingMarkdownPresentation` 只管理 canonical、可见 offset 和不足一个字符的速率 carry。每个组件至多存在一个 RAF；新快照替换 canonical，不创建并行任务。流式过程中不因积压量跳过逐字展示；流结束时按最高展示速度估算剩余追赶时间，500 ms 内继续推进，超过 500 ms 则直接收敛到最终文本，避免长积压在生成结束后继续占用渲染资源。
3. Streamdown 继续接收逐字增长的 Markdown 前缀并修复 incomplete Markdown；`createIncrementalMarkdownBlockParser` 缓存已经完成的块，追加内容时只把上一个可变尾块送回 block lexer。已完成的段落、列表和代码块保持相同内容与索引，由 Streamdown `Block` memo 跳过解析和 DOM reconciliation。
4. 非追加改写才回退为一次全量 block parse；这是正确性兜底，不是正常流式路径。
5. 已完成消息通过稳定字符串 props 与 `React.memo` 保持引用稳定；工作区 tabs、width、activeTab 变化不得穿透命令 context 触发历史消息更新。

性能缺陷不在“流式 Markdown”本身，而在“每帧重新处理整条累计消息”。新设计保留旧版打字机语义，把每帧 Markdown 工作量约束在当前可变尾块，避免处理成本随历史正文总长度持续增长。

## 生命周期与接口不变量

- 数据层：ACP cumulative snapshot 是唯一持久 canonical text；presentation 只保存当前 canonical 引用和数值 offset/carry，不生成历史快照队列。
- 调度层：每个 Markdown 实例至多存在一个可取消 RAF；canonical 更新只替换目标，不能叠加多个逐字任务。
- 解析层：append-only 更新只重算上一可变尾块；稳定块不得再次进入 block lexer。
- 展示层：可见前缀始终由 Streamdown 按 Markdown 渲染，不允许先显示完整静态文本再隐藏重播。
- 收敛层：`streaming=true` 时始终保留自适应逐字节奏；`streaming=false` 时，剩余追赶时间不超过 500 ms 则继续逐字完成，超过 500 ms 则一次性展示最终 canonical Markdown。剩余量按 Unicode code point 计算，不能把 emoji 等代理对误算为两个字符。
- 完成层：静态 Markdown 不订阅右侧工作区展示状态，只通过稳定 `openResource` 命令处理链接。
- 清理层：组件卸载或任务替换时取消唯一 RAF，不保留累计前缀版本。

## 回归验收

- 打字机必须从首个可见字符开始推进，不能先显示完整文本再隐藏。
- 新 canonical 到达时只能替换当前任务；任意时刻至多存在一个 pending RAF，卸载后为零。
- 逐字前缀始终保留 incomplete Markdown 修复；流结束后必须收敛为完整 canonical Markdown。
- 流结束时剩余 90 个字符（180 字符/秒下为 500 ms）必须继续逐字完成，剩余 91 个字符必须立即收敛；流未结束时即使积压更大也不得跳过。
- append-only 长消息的 block lexer 输入不得重复包含已经稳定的历史块；非追加改写必须正确回退全量解析。
- 打开 15 个文件时历史消息不重渲染。
- 6021 帧回放仍只保留每个 identity 的最新累计文本。
- 真实开发版复测必须记录 baseline / target / final 的 V8、embedder、DOM、Renderer private bytes，并用 memlab 确认 detached DOM、CodeMirror 和累计字符串不存在大 retained chain。
