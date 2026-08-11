# 流式 Markdown 原生内存高水位修复方案

## 目标

保留 ACP 流式消息原有的逐字 Markdown 体验，同时消除每个 32 ms 帧重复处理整条累计消息的问题，使每帧解析范围只包含当前可变 Markdown 尾块。

## 实施项

### 根因修复

- [x] 恢复 `StreamingMarkdownPresentation` 的 canonical / offset / carry 逐字语义和原有自适应速率。
- [x] 每个流只允许一个可替换 RAF；canonical 快照更新不创建并行队列。
- [x] 保持 Streamdown streaming mode，使逐字前缀始终按 Markdown 渲染并支持 incomplete Markdown。
- [x] 新增 append-only block parser cache，只重新 lex 上一个可变尾块，稳定历史块保持不变。
- [x] 删除会造成“完整文本闪现、隐藏、重播”的 CSS 字符动画和完成态收尾切换。
- [x] 保留 incomplete Markdown 解析和流结束后的最终静态结果。
- [x] 增加流结束收敛阈值：预计 500 ms 内可追完则保留打字机，超过则立即展示最终 Markdown；流式过程中不跳过。

### 回归测试

- [x] 固化“任意时刻只有一个 pending RAF，快照替换和卸载会取消旧任务”。
- [x] 固化逐字前缀、Markdown 控制符归一化及最终 canonical 收敛。
- [x] 固化结束阈值边界：剩余 90 个 Unicode 字符继续逐字、91 个立即收敛，且流未结束时大积压仍保留逐字展示。
- [x] 固化 append-only 只重算可变尾块、历史改写回退全量解析。
- [x] 保留不完整 Markdown、链接代理及代码 DOM 契约测试。
- [x] 保留“打开 15 个文件时历史 Markdown 不重渲染”测试。
- [x] 真实逐字 Markdown 回放确认首帧只显示首字符且 `<strong>` 已生效；相关接口测试 14/14、完整前端测试 804/804、生产构建通过。

### 真实开发版验收

- [x] 修复前采集 baseline / target / final 与 memlab retained chain。
- [x] 开发版重启后基线恢复为 V8 27.24 MB、embedder 9.76 MB、4,168 DOM nodes，CDP 入口持续可用。
- [ ] 确认长流式输出期间每帧只解析当前可变尾块，不重复解析稳定历史块。
- [ ] 确认工作区标签关闭后不存在 CodeMirror、累计字符串或大型 detached DOM retained chain。

## 验收标准

接口层以调度单飞、结束收敛和解析范围为硬约束：任意时刻只能有一个逐字 RAF，流结束后的预计追赶时间以 500 ms 为边界，append-only 更新只能把可变尾块交给 block lexer，历史消息不因工作区操作重渲染。现场层不要求 Windows 立即归还所有 Chromium allocator 高水位页，但 target 的增长速度必须显著下降，且 final 的 JS/Blink 活对象能够回落；若原生 private bytes 仍持续单调增长，下一阶段使用 CDP native memory sampling 从应用启动前采样，而不是把任务管理器数值误判为 JS heap 泄漏。
