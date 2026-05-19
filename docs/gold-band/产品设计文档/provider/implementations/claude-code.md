# Claude Code Provider 实现

## 1. 定位
`claude-code` 是 Gold Band 当前默认的 provider implementation。

它的职责不是再做一层 runtime 级输入整理，而是消费 runtime 已经准备好的 `prompt bundle`，并在 runtime / adapter 提供的执行环境中把它映射到 Claude Code 的具体调用方式。

换句话说：
- A() 是 runtime 拥有并直接依赖的稳定接口
- B() 是 Claude Code implementation 负责实现的内部执行接口
- 二者可以同处 provider 模块，但 ownership 不同
- invocation 解析、热/冷数据选择、`prompt bundle` 组装，属于 runtime 层
- Claude Code implementation 负责执行 provider implementation 的 **B()**

---

## 2. Claude Code implementation 的职责边界
Claude Code implementation 负责：
- 接收 runtime 已准备好的 `prompt bundle`
- 根据 runtime / adapter 提供的执行上下文选择 new / continue 调用方式
- 在 runtime / adapter 指定的工作目录中启动 Claude Code
- 让模型可按需读取 runtime 已暴露的冷数据文件
- 收集 Claude Code 的最终输出
- 提取 `primaryArtifact` 原材料（若当前节点声明了它）
- 收集 raw stream（若启用）
- 产出 `workerRefSeed`
- 返回 provider 统一输出包给 runtime

Claude Code implementation 不负责：
- 解析 workflow / node DSL
- 决定本次 invocationKind
- 选择哪些 artifacts / attachments 暴露给模型
- 组装 `systemPrompt` / `userPrompt`
- 校验 `primaryArtifact` 的 schema
- canonical artifact 落盘
- `worker-ref.json` 最终写盘

这些职责都属于 runtime。

---

## 3. 已知基础映射

### 3.1 provider id
- `claude-code`

### 3.2 worker reference
Claude Code 首版最小继续引用建议使用：
- `sessionId`

### 3.3 打开 / 继续会话
当前典型命令模板可表现为：

```bash
claude -c <session_id>
```

说明：
- 这里的命令模板只表达 Claude Code 的 provider-specific 会话继续方式
- 它不等同于 Gold Band CLI 层的 `continue` / `retry` 命令语义

### 3.4 流式输出
Claude Code 当前以 `--output-format stream-json` 作为首选流式协议。

说明：
- provider 会把 Claude Code stdout 的每条 stream-json 事件原样旁路写入 `raw.stream.jsonl`
- provider 当前会把调用 provider 的输入快照写入 `progress.events.jsonl`，用于记录本次 invocation 输入上下文；若发现旧文件中残留了非 `provider_input` 内容，会在流读取前先清理
- 结构化输出约束只来自当前节点 `output` DSL，并明确写入 system prompt，而不只是在 task/user prompt 中隐含约束
- 不再根据 `exec-plan`、`verify-result` 等 artifact 名称自动向 system prompt 注入内置输出契约
- `raw.stream.jsonl` 仍属于 provider-specific 原始观测面
- `progress.events.jsonl` 仍保留为 Gold Band 的 provider-agnostic 过程观测面路径；Claude Code 输出流的正式规范化解析留待后续实现
- 两者都不应直接成为 Gold Band 稳定控制流的依据
- 若 provider 进程启动失败或以非零状态退出，错误需要直接返回 runtime；runtime 将当前 attempt / run 标记为 blocked，而不是让 console 长时间停留在 `calling-provider`
- Windows 下若 Claude Code 缺少 Git Bash / `CLAUDE_CODE_GIT_BASH_PATH`，provider 错误信息应显式带出该提示

---

## 4. B() 的最小输入
Claude Code implementation 接收的应是 runtime 已经准备好的 `prompt bundle`。

首版建议至少包含：
- `systemPrompt`
- `userPrompt`
- `metadata`

建议最小示意：

```json
{
  "systemPrompt": "<rendered system prompt>",
  "userPrompt": "<rendered user prompt>",
  "metadata": {
    "profile": "developer",
    "nodeType": "worker",
    "primaryArtifact": "dev-result"
  }
}
```

### 字段语义
- `systemPrompt`：runtime 已渲染好的 system prompt 正文
- `userPrompt`：runtime 已渲染好的 user prompt 正文
- `metadata`：辅助信息；用于 provider implementation 记录与调试

说明：
- `systemPrompt` / `userPrompt` 已经是最终模型输入层；Claude Code implementation 不再改写其结构职责
- `workspaceDir`、`sessionMode`、`continueRef`、`streamMode` 属于 runtime / adapter 提供的执行上下文，不属于 B() 的语义输入负载
- Claude Code implementation 可以在 provider-specific 适配上加少量封装，但不应重新发明一套 prompt 契约

---

## 5. Claude Code 的输入映射

## 5.1 prompt 输入映射
首版建议 Claude Code implementation 按以下原则映射输入：

- `systemPrompt` 保持为稳定运行约束层
- `userPrompt` 保持为本次任务正文层
- Claude Code implementation 不重新决定哪些内容属于 system 或 user
- 若 Claude Code 底层接口不支持显式 system/user 双通道，则 provider implementation 需要保留这两层语义并做最小损失映射

实现原则：
- 优先保留 `systemPrompt` 的约束优先级
- 不把冷数据正文重新展开到输入中
- 不让 provider implementation 擅自补充 runtime 未显式暴露的上下文

## 5.2 冷数据访问映射
Claude Code implementation 不直接内联冷数据正文。

它应依赖以下前提：
- Claude Code 运行在 runtime / adapter 指定的工作目录中
- runtime 暴露的冷数据文件路径对 Claude Code 可读
- 模型通过 prompt 中的冷数据索引按需读取文件

说明：
- Claude Code implementation 的职责是保证这些路径在其工作环境中可达
- Claude Code implementation 不负责重新挑选哪些冷数据应暴露

## 5.3 attachments 目录规则
Claude Code implementation 不需要单独接收 `attachmentsDir` 字段。

原因是：
- 对模型而言，附件允许写入的位置最终通过 `systemPrompt` 感知
- `attachments_dir` 属于 runtime 在渲染 prompt 时注入的运行约束变量
- Claude Code implementation 只消费已经渲染好的 prompt，而不再重新组装这些模板变量

因此：
- provider implementation 不应把附件目录规则当作自己的结构化输入字段
- 附件目录约束应稳定体现在 `systemPrompt` 中

---

## 6. 会话模式映射

## 6.1 `sessionMode = new`
表示本次调用以新会话执行。

Claude Code implementation 应：
- 启动新的 Claude Code 会话
- 不依赖历史 `sessionId`
- 在完成后尝试提取新的 `sessionId`

## 6.2 `sessionMode = continue`
表示本次调用尝试继续历史会话。

Claude Code implementation 应：
- 从 `continueRef` 中读取 `sessionId`
- 使用 Claude Code 的 continue 方式启动
- 若缺少必要 `sessionId`，则 provider invocation 应失败并给出明确原因

建议最小继续命令模板：

```bash
claude -c <session_id>
```

## 6.3 首版默认策略建议
Claude Code implementation 本身不决定策略，但建议 runtime 在调用 Claude Code 时默认采用：

- 未显式提供 `sessionMode`：默认 `new`
- 只有 workflow edge 明确声明 `session = continue` 时才复用历史会话
- 如果跨节点需要新的角色、文件规则或 output DSL 约束，应使用新 session

原因：
- ACP 的 system prompt 只在 `session/new` 注入
- 首版优先保证输入边界清晰，而不是追求会话复用率

---

## 7. 结果提取策略
Claude Code implementation 需要把 Claude Code 的底层结果提取并归一成 Gold Band provider 输出。

## 7.1 首版推荐策略
首版建议采用：
- `primaryArtifact` 走**最终文本提取**
- `attachments` 走**文件副作用**

也就是：
- 若当前节点声明了 `primaryArtifact`，则 Claude Code implementation 需要从最终响应中提取该 artifact 的 `name` 与 `content`
- 这里的 `content` 是模型按 output structure 返回的原始内容字符串
- Claude Code implementation 不负责把这段内容 parse 成 JSON 或其他语义对象
- 其他自由格式材料允许作为执行过程中写出的文件副作用留在 `attachments/`

这样可以同时满足：
- canonical artifact 仍由 runtime 统一规范化
- 自由附件不必强行塞进结构化返回

## 7.2 为什么不建议首版把 canonical artifact 直接交给文件副作用
若把 `primaryArtifact` 完全依赖文件副作用：
- provider implementation 与 runtime 的边界会变模糊
- runtime 更难区分“最终标准输出”与“执行过程附带文件”
- canonical artifact 的提取、错误归因与一致性会更难做

因此首版建议：
- `primaryArtifact` 的来源仍然以 provider 返回文本提取为主
- runtime 再统一校验与落盘

## 7.3 最小输出包
Claude Code implementation 返回给 runtime 的最小输出应仍满足通用契约：

```json
{
  "status": "completed",
  "exitCode": 0,
  "resultPayload": {
    "primaryArtifact": {
      "name": "exec-plan",
      "content": "{ ... }"
    }
  },
  "workerRefSeed": {
    "sessionId": "4aefdd5f-1b5c-47d0-92a3-69005afb53f9"
  },
  "stream": null
}
```

说明：
- 若当前节点未声明 `primaryArtifact`，则 `resultPayload` 可为空或缺省
- `workerRefSeed` 是 provider-specific 原材料，不是最终 `worker-ref.json`
- 最终 `worker-ref.json` 由 runtime 落盘

---

## 8. workerRefSeed 映射
Claude Code implementation 首版建议返回：

```json
{
  "sessionId": "4aefdd5f-1b5c-47d0-92a3-69005afb53f9"
}
```

runtime 在后处理时再把它扩展为 canonical `worker-ref.json`，例如：

```json
{
  "version": "0.1",
  "provider": "claude-code",
  "mode": "new",
  "supportsOpenSession": true,
  "supportsContinueSession": true,
  "continueRef": {
    "sessionId": "4aefdd5f-1b5c-47d0-92a3-69005afb53f9"
  },
  "openCommand": {
    "command": "claude -c 4aefdd5f-1b5c-47d0-92a3-69005afb53f9"
  }
}
```

说明：
- provider implementation 返回 seed
- runtime 负责 canonical 包装
- 这样 provider-specific 差异与 runtime 统一格式可以同时保留

---

## 9. `doctor()` 检查项
Claude Code implementation 的 `doctor()` 首版建议最少检查以下内容。

## 9.1 基础可执行性
- Claude Code CLI 是否已安装
- `claude` 可执行入口是否存在
- 最小调用是否能成功启动

## 9.2 会话能力
- 是否支持继续已有 session
- 是否支持生成可打开的会话引用
- 若 workflow edge 显式请求 `session = continue`，但当前环境不支持 continue，应在 DSL / runtime 校验阶段直接报错
- 若当前环境不支持打开原始会话，应在 capability 中表达 `supportsOpenSession = false`，并由 CLI `open-session` 明确报错

## 9.3 工作目录能力
- Claude Code 是否可在指定 `workspaceDir` 中工作
- Claude Code 是否可读取 runtime 暴露的冷数据文件路径

## 9.4 流式输出能力
- 是否支持 raw stream 采集
- 若 stream 能力不可用，不应影响主执行能力

## 9.5 结果提取能力
- 是否具备稳定提取最终文本输出的能力
- 若当前环境下无法稳定提取 `primaryArtifact`，应明确报告风险或降级能力

---

## 10. capability matrix
首版建议 Claude Code implementation 至少暴露以下能力：

```json
{
  "supportsNewSession": true,
  "supportsContinueSession": true,
  "supportsOpenSession": true,
  "supportsRawStream": true,
  "supportsColdFileAccess": true,
  "supportsPrimaryArtifactTextReturn": true,
  "supportsAttachmentSideEffects": true
}
```

说明：
- capability 用于告诉 runtime 这个 provider implementation 能做什么
- runtime 可据此决定是否允许某些执行模式或是否需要降级
- 若 `supportsContinueSession = false`，则任何显式请求 `session = continue` 的 DSL 都应视为校验错误，而不是静默降级
- 若 `supportsOpenSession = false`，CLI `open-session` 应明确报错
- 若 `supportsRawStream = false`，progress 应退化为 polling / 状态快照 / 最终快照，而不是影响主执行

---

## 11. raw stream 与 progress 的关系
Claude Code implementation 若启用了 `streamMode = raw`，建议：
- 尽量原样保留 Claude Code 的 provider raw stream
- 将其写入 `raw.stream.jsonl` 对应的数据源
- 再由 runtime 或上层观测组件决定是否提炼为 `progress.events.jsonl`

这里应坚持的边界是：
- raw stream 是 provider-specific 事实流
- progress.events / run-progress.json 是 Gold Band 规范化观测层
- Claude Code implementation 不必承诺直接产出最终规范化 progress

说明：
- 首版可以只保证 raw stream 留档
- 若当前环境不支持 raw stream，则应退化为 polling / 状态快照 / 最终快照模式
- progress 规范化可作为后续增强层逐步补上

---

## 12. 推荐执行流程
Claude Code implementation 的 `runWorker()` 首版推荐流程如下：

1. 接收 runtime 提供的 `prompt bundle` 与执行上下文
2. 校验执行上下文中的 `sessionMode` 与 `continueRef` 是否匹配
3. 在 runtime / adapter 指定的工作目录中启动 Claude Code
4. 将 `systemPrompt` / `userPrompt` 映射为 Claude Code 调用输入
5. 若执行上下文要求 `continue`，按 `sessionId` 继续会话
6. 若执行上下文要求 raw stream，则同步采集 provider 原始流
7. 等待 Claude Code 完成
8. 提取最终文本结果
9. 若声明了 `primaryArtifact`，则从最终结果中提取 `primaryArtifact` 原材料
10. 提取 `sessionId`，生成 `workerRefSeed`
11. 返回统一 provider 输出包给 runtime

---

## 13. 当前仍刻意留白的点
首版先不在此文档中完全写死：
- Claude Code 最终文本结果的精确抽取语法
- raw stream 到 `progress.events.jsonl` 的完整映射规则
- Claude Code 不同运行模式下的全部命令参数矩阵
- 更复杂的多 session 复用策略

这些内容建议等首版实现跑通后，再根据真实行为收敛。

---

## 14. 相关文档
- [Provider 概览](../overview.md)
- [Provider Adapter 接口](../adapter.md)
- [Worker Invocation Contract](../invocation.md)
- [Prompt Bundle 规范](../prompt-bundle.md)
- [Worker Ref 规范](../worker-ref.md)
- [Progress 规范](../../interaction/progress.md)

---

## 15. 一句话总结

> Claude Code implementation 是 Gold Band provider 体系中的默认 B()：它不负责重新组织 runtime 输入，而是负责消费 runtime 已准备好的 `prompt bundle`，把它稳定映射到 Claude Code 的会话、输入、结果提取与观测能力上；其中 `primaryArtifact.content` 只保留模型返回的 raw content。