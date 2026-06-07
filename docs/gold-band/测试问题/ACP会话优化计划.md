# ACP 会话窗口优化计划

## 1. 会话窗口滑动逻辑

问题：
- Chat UI 滑动到底部后切换“原始帧”，Raw frames 视图会自动滑到页面底部。
- Chat UI 滑动到底部后点击工具展开，不是原地展开，而是展开后自动滑到底部，阅读体验不连贯。

处理结果：
- ACP 会话容器只在新增 ACP event 或发送用户 prompt 时按“保留用户阅读位置”规则显式贴底。
- 切换 Raw frames、展开 raw frame、展开 tool call 时捕获并恢复当前滚动位置，不再触发自动滑到底部。

验收：
- 在底部切换 Raw frames，不出现额外滑动。
- 展开工具卡片时保持当前阅读位置。

## 2. 工具调用和思考过程头像

问题：
- 工具调用、思考过程没有和普通 agent 文本一样展示机器人头像。

处理结果：
- thought、tool call、plan、处理中状态统一包裹为 assistant 时间轴行，但不展示机器人头像。
- 结构化行保留原工具卡横向位置，避免去掉头像后卡片突然左移。
- 工具卡标题改为左对齐单行，显著展示操作名，次级展示路径、pattern 或 query，例如 `Glob .claude/**/*`、`Read xxx.js`。
- 用户 prompt 仍保持右侧用户头像。

验收：
- Agent 文本继续展示机器人头像；思考过程、工具调用、计划块和处理中状态不展示头像，但位置保持稳定。

## 3. 页面自适应问题

问题：
- 小屏或窄抽屉下，会话窗口内容存在宽度溢出。

处理结果：
- 会话抽屉宽度调整为 `min(760px, calc(100vw - 32px))`。
- ACP 文本、thought、plan、raw frame 和工具输入输出补齐 `min-w-0`、`max-w-full`、主动换行和内层滚动约束。

验收：
- 375px / 760px 宽度下，会话抽屉和长文本不出现横向撑宽。

## 4. 工作流 run 展开后页面超宽

问题：
- 工作流 run 展开后，页面内容超出宽度，显示不完整。

处理结果：
- 重点收紧节点详情会话抽屉和 ACP 工具卡宽度，避免会话内容撑宽工作台。
- 工具输出、raw JSON、长路径和连续字符在自身容器内换行或滚动。

验收：
- 展开 run / 会话 / 工具输出后，工作台不出现由 ACP 内容导致的横向溢出。

## 5. 停止会话体感

问题：
- 工作流列表里的停止 run 几乎立即生效，但 ACP 会话窗口内停止会话会长时间停留在“停止中”，体感明显更慢。

处理结果：
- 点击 ACP 停止后，先立即把当前 attempt / run / round 收敛为 `paused + process_interrupted`，让会话抽屉和 round 详情同步退出 active 态。
- ACP runtime 观察到取消标记后发送不带 `id` 的 `session/cancel` notification，请求 provider 优雅退出；若短暂宽限后 provider 仍未退出，再强制 kill `provider.pid` 对应进程树。
- stale cancel fuse 仍保留 15 秒兜底，确保异常情况下 session 最终会写成 `cancelled`，不会永久卡在 stopping。

验收：
- 在 ACP 会话窗口点击停止后，composer 很快退出 stopping，round 详情很快变为 paused / 可继续。
- provider 能优先优雅退出；只有超时未退出时才触发 kill fallback。

## 6. 初始状态、流式反馈与计时

问题：
- 刚开始显示“暂无 ACP 事件”，实际应该是 Claude 调起中。
- 当前会话窗口是一整段完成后才出现；模型长时间无返回时页面没有变化，用户无法判断是否卡死。
- 需要从发起信息到首帧返回之间展示处理中标识，并增加当前处理计时和总耗时。

处理结果：
- pending / running 且 timeline 为空时显示“Claude 调起中”。
- 用户点击发送后到 `session/prompt` 请求完成前，在 composer 输入框内部显示“发送中”动效；消息已发出后到首个非用户帧前切换为“处理中”动效，但发送等待阶段不计入总耗时。
- 首帧返回后根据最新可见事件类型，在 composer 内显示“思考中 / 工具调用中 / 回复生成中”。
- composer 状态展示当前步骤耗时和总耗时；当前步骤计时只在处理阶段显示，总耗时按每轮“首个非用户响应帧到该轮响应结束”的耗时累加，两轮之间的用户空闲时间不计入总耗时，会话结束后继续常驻展示，消息流不再插入独立处理中卡片。
- 继续 ACP session 时，`session/load` 回放的历史上下文不重复追加到 UI 事件流，避免继续会话次数越多、历史消息和总耗时越膨胀。
- Raw frames 作为 JSONL 诊断日志后端分页读取，默认加载最新页，支持关键词检索、direction 和 kind/method 过滤，不全量传给前端。
- ACP Chat 普通 session 查询默认只加载最近约 30 条后端归一化 UI events；后端用文件行序号作为稳定游标，并先合并连续 text / thought delta，避免历史窗口从流式回复中间截断；向上滚动到顶部再按 `beforeSeq` 加载更早窗口，向下回看被裁剪的较新内容时按 `afterSeq` 静默取回；message list 使用虚拟长列表，前端只保留有限事件窗口；session metadata、pending permission、usage 与 diagnostics 由后端流式统计，轮询不返回完整 `acp.events.jsonl`。
- 返回给会话主 UI 的 raw payload 只保留摘要字段并设置截断上限；完整原始内容仍通过 Raw frames 分页诊断。
- ACP stdout 读取链路使用有界队列，避免 adapter 输出速度超过处理速度时在进程内无界堆积。
- 权限请求不使用大块表单卡片，改为类似 prompt-kit system-message 的轻量 inline action bar：左侧权限图标与标题，右侧紧凑操作按钮。

验收：
- 新会话调起阶段不再显示“暂无 ACP 事件”。
- 发送 prompt 时先看到发送中动效；消息发出但模型暂未返回内容时切换为处理中动效和计时；agent 回复追加后消息区自动跟到底部，处理状态结束只移除处理中提示，不发生跳顶部再回底部。
- 工具调用、思考和回复生成阶段能显示对应状态。
- 长会话打开和轮询时，WebView 不再因完整历史反复加载而随总事件数线性膨胀；顶部加载更早消息时保持当前阅读位置，向下回看较新内容时不展示额外加载文案或动画。
