# `verify` 节点规范

## 1. 当前定位
`verify` 节点负责独立语义验收。

它在 DSL 层是独立节点类型；但在执行层，应视为一种**固定职责的特殊 `worker`**：
- 不负责实现需求
- 不负责执行命令
- 只负责基于 runtime 显式组装的证据，判断“是否通过验收”

## 2. 当前已知结论
- 判断任务是否真的完成
- 判断当前验证证据是否充分
- 不直接决定大循环是否继续，失败后的控制策略由 `onAcceptanceFailure` 决定
- 首版建议复用 provider 的 `runWorker()` 执行通道，而不是单独设计另一套 provider 调用接口
- `verify` 的 canonical artifact 固定为 `verify-result`

说明：
- DSL 上，`verify` 仍然是独立节点类型，而不是把它直接写成通用 `worker`
- Runtime / provider 执行上，可以把它当作“固定职责的特殊 worker”处理
- 因此它在 layout 上接近 `worker` attempt，但其输入和输出职责更收敛

## 3. 标准输入如何自动组装
`verify` 不应自己去扫整个 run / round / attempt 目录。

正确做法应是：
- runtime 显式组装一份最小验收输入包
- provider 只消费这份输入包
- `verify` 只对被显式提供的证据负责

### 3.1 最小输入原则
`verify` 的最小输入应至少包括：
- 原始 requirement
- 当前 round 的关键执行证据
- runtime 显式暴露的补充附件引用

### 3.2 MVP 默认输入包
首版直接固定为以下最小验收输入包：

1. **原始 requirement**
   - 来源：task 的稳定 requirement
   - 作用：判断“目标是否已满足”

2. **当前 round 最新 `exec-result`**
   - 作为首版默认的执行证据
   - 若当前 round 内有多次 `exec` attempt，则以最新 attempt 为准

3. **当前 round 最新上游 worker primary artifact**
   - 作为首版默认的实现侧证据
   - 若当前 round 内存在多个与验收相关的上游 worker 证据源，则由 runtime 按 DSL / 控制流显式决定纳入哪个 primary artifact
   - 同一证据源按最新 attempt 优先

4. **runtime 显式选中的附件引用**
   - 例如 `attachments/` 下的人类可读分析、测试说明、补充报告
   - 这些附件只能由 runtime 显式选入上下文
   - `verify` 不应默认扫描整个 `attachments/` 目录

5. **最小运行时上下文摘要**
   - 例如当前 task / run / round / node 的基础标识
   - 当前轮的验收目标说明
   - 必要时可包含 workflow 中与验收相关的最小上下文

### 3.3 明确排除项
首版建议 `verify` 默认**不直接依赖**：
- provider raw stream 全量内容
- 整个 workspace 的无边界自由扫描结果
- 整个 run 历史 round 的全部上下文
- 未被 runtime 显式暴露的附件或 sidecar 文件

补充规则：
- 证据选择范围只限当前 round
- 默认不跨 round 回溯旧证据
- 默认不把上一轮完整证据包再次展开给新一轮 `verify`

### 3.4 输入组装责任边界
- requirement 的读取与整理，由 runtime 完成
- artifact 的解引用与选择，由 runtime 完成
- attachments 的筛选与引用，由 runtime 完成
- provider 只接收已经准备好的验收 prompt bundle
- `verify` 只对这份输入包做验收判断

## 4. 如何表达 unmet requirements / validation gaps
`verify-result` 中这两类字段应表达两种不同失败语义。

### 4.1 `unmetRequirements`
表示：**需求本身尚未满足**。

典型例子：
- 功能没有实现完整
- 明确要求的场景没有覆盖
- 行为与 requirement 不一致

也就是说，它回答的是：
> “东西做完了吗？”

### 4.2 `validationGaps`
表示：**当前证据不足以支持通过验收**。

典型例子：
- 只运行了局部测试，缺少关键链路验证
- 缺少必要日志、截图、报告或命令结果
- 有实现迹象，但无法确认是否真正满足 requirement

也就是说，它回答的是：
> “就算看起来做了，我们有足够证据相信它做对了吗？”

### 4.3 两者关系
- 两者可以同时存在
- `unmetRequirements` 偏向“实现未完成或不正确”
- `validationGaps` 偏向“证据不足，无法放行”

首版建议：
- `status = success` 时，这两个数组都应为空
- `status = failure` 时，至少应有一个数组非空

## 5. 如何与 `onAcceptanceFailure` 协同
`verify` 本身只负责产出验收结论，不负责决定下一步控制流。

边界应明确为：
- `verify` 负责产出 `verify-result`
- runtime 根据 `verify-result.status` 将当前节点归纳为 `success` 或 `failure`
- 控制层再根据 `onAcceptanceFailure` 决定：
  - `auto_loop`
  - `stop`

### 5.1 `verify.success`
- 当前 run 通过验收
- runtime 可直接结束 run，或流转到 workflow 的终止态

### 5.2 `verify.failure`
- 只表示“验收未通过”
- 不等于自动继续下一轮
- 是否进入下一轮，只由 `onAcceptanceFailure` 决定

### 5.3 进入下一轮时的反馈来源
若 `onAcceptanceFailure` 使 workflow 进入下一轮，则：
- task 的原始 requirement 不改写
- 最新 `verify-result` 作为下一轮修复反馈输入
- 新一轮 `worker` 应消费“原始 requirement + 最新 verify-result”

## 6. 实现建议
首版实现时，建议把 `verify` 当成“固定职责的特殊 worker”处理：
- 走与 `worker` 相同的 provider invocation 主通道
- 使用独立的节点类型与 artifact 语义
- 输入由 runtime 显式组装
- 输出固定收敛为 `verify-result`

这样可以同时满足：
- DSL 语义清晰
- runtime 实现复用度高
- provider 边界不被额外复杂化

## 7. 相关文档
- [DSL 概览](../overview.md)
- [verify-result](../artifacts/verify-result.md)
- [Worker Invocation Contract](../../provider/invocation.md)
- [Prompt Bundle 规范](../../provider/prompt-bundle.md)
- [Runtime Control](../../runtime/control.md)
