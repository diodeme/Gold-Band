# UI 文案规则

本文件是 Gold Band 界面文案的语义来源、译法和占位符约束。提示词正文遵守 `prompt-authoring.md`，不在本文件规定。

## 1. 语义来源

- `web/src/locales/zh-CN.json` 是界面文案的语义来源。
- `en` 只用来消除歧义。zh-CN 与 en 冲突时，先核对产品行为，再改文案。
- 按钮、状态、标题和说明按各自的语气翻译。同一个概念全文只用一种译法。

## 2. 保留词与占位符

- zh-CN 保留的英文产品词原样保留，包括 Agent、Git、Workflow、MCP、ACP、CI/CD、worktree，以及该条 zh-CN 里出现的其他拉丁专名。
- zh-CN 没有保留的词不要强行写成英文。
- `{{placeholder}}` 的名字和数量必须与 zh-CN 一致。
- key 对齐、占位符一致和上述保留词由 `web/tests/i18n-loading.test.ts` 的 catalog contract 检查。

## 3. 繁体中文

- zh-TW 按台湾产品用语校对，不只做简繁转换。
- 文件系统对象用檔案，说明文档用文件。

## 4. 翻译方式

- 不要只用无上下文的逐条机翻脚本更新 `web/src/locales/`。
- 短字符串必须能判断它在界面里的词性和语气，再决定译法。
