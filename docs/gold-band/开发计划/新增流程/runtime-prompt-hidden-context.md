# Runtime prompt 拆分与 hidden context

## 目标

ACP `session/load` / resume 不能可靠刷新动态 system prompt：Claude ACP 在相同 `cwd + mcpServers` 下会复用已有 session，Codex ACP 当前不消费 `_meta.systemPrompt`。因此 Gold Band 将 prompt 拆为稳定规则和每次 invocation 上下文两部分。

## 规则

- 稳定规则继续放在 `systemPrompt`：节点角色、output contract、文件写入规则、profile、现有 `extra_system_sections`。
- 每次 workflow invocation 的运行事实放在 user prompt hidden 段：session mode、round、attempt、attempt/attachments 目录、前序链、前序产物摘要、workflow resume 反馈。
- prompt 渲染使用显式 render mode 区分 `RequirementTask`、`WorkflowResume`、`RuntimeRepair`、`UserMessage`，不能只根据 `SessionMode::Continue` 判断。
- `RequirementTask` 与 `WorkflowResume` 注入 Gold Band hidden runtime context；`RuntimeRepair` 和 `UserMessage` 不注入 hidden context。
- workflow continue/resume 不重传完整原始 user prompt；可见正文只发送简短 `Goal`。
- runtime repair 是紧接输出校验失败后的内部修复 turn，只发送 repair prompt 原文。
- 用户在 stopped/completed ACP session 中手动继续或追问时，只发送用户原文，不包 `# Goal` / `# Requirement`；这同样适用于 AI-DYNAMIC 内部 leaf 的人工继续，不因为 `SessionMode::Continue` 就自动回到 `WorkflowResume`。
- AI-DYNAMIC 外层 run 暂停后，带用户显式输入的 parent continue 必须向下传给被恢复的 workflow-invocation child run，并在 child worker 上保持 `UserMessage`；只有无用户输入的纯恢复才渲染 `WorkflowResume` hidden context。
- AI-DYNAMIC 内部 bootstrap / worker / merge / acceptance 与普通节点共用 workflow runtime render mode：新节点和新 attempt 使用 `RequirementTask`，只有真实继续已有 session 且没有用户显式输入时才使用 `WorkflowResume`；新建 merge / acceptance 不能因为带有动态上下文而渲染成 hidden context + `# Goal`。
- runtime prompt 不再注入 `skill_catalog`。
- user prompt 不再渲染 `Cold Artifact Index` / `Cold Attachment Index`。

## hidden 格式

```html
<hidden data-gold-band-hidden="true" title="Gold Band runtime context">
...
</hidden>
```

不支持 system prompt 的 provider 会额外收到：

```html
<hidden data-gold-band-hidden="true" title="Gold Band stable system prompt">
...stable system prompt...
</hidden>
```

## Provider capability

`ProviderCapabilities` 新增 `supports_system_prompt`。

- `claude-acp`: true
- `codex-acp`: false
- 其他 provider 默认 false，除非确认 adapter 支持 `_meta.systemPrompt.append`

`~/.gold-band/desktop/agent-diagnostics.json` 只保存 doctor/handshake 诊断结果，不作为静态能力来源。

## 前端展示

- 新会话态和旧工作台都复用 ACPChatDialog，因此统一接入 hidden prompt renderer。
- hidden 段和可见用户消息在同一个 user bubble 内展示。
- hidden 后面的可见片段只在展示层压缩开头空行，真实 prompt event 与 adapter 发送内容不变。
- 默认折叠，显示标题和字符数；展开后显示原文，再次点击收起。
- 只折叠带 `data-gold-band-hidden="true"` 的 Gold Band hidden 段；普通 `<hidden>` 文本保持可见。
