# PR 提交流程

## 目标

把“提交 commit”和“提交 PR”拆成两个内部 skill 的职责边界，但对实际使用者保持单入口：

- `git-commit` 只负责 commit message、staging 和 commit 边界
- `git-pr` 负责完整 PR 发布流程，并在需要时先串起 `git-commit`
- 用户在需要发 PR 时，只需要触发一次 `git-pr`

## 为什么要拆开

本仓库已经存在两套不同层级的规范：

- commit 规范：Conventional Commits
- PR 规范：仓库可能通过 `.github/workflows/semantic-pr.yml` 等 CI 规则强制校验 PR title

如果只用 commit skill 直接外推 PR 标题，容易出现：

- commit message 合规
- PR title 不合规
- push 成功但 CI 因标题失败

因此需要把 PR 阶段作为独立流程处理。

## 推荐使用方式

### 1. 只提交 commit

使用 `git-commit`：

- 分析 diff
- 选择本次提交范围
- 生成 conventional commit message
- 创建 commit

### 2. 提交 PR

使用 `git-pr`，不要手动先后拼接多个 skill。

`git-pr` 内部应完成：

1. 检查当前工作区和分支状态
2. 如果目标改动尚未提交，先切到 `git-commit`
3. 检测仓库 PR 标题规则
4. 选择可写远端并完成 push
5. 创建或更新 PR
6. 观察 PR checks，至少确认标题校验结果

## PR 标题规则

### 有 semantic PR title 规则时

优先生成 conventional 风格标题，例如：

- `fix: route conversation workspaces correctly`
- `feat: add agent diagnostics retry banner`
- `docs: clarify conversation workspace routing`

不要使用：

- `[codex] ...`
- `update ...`
- `fix bug`

### 没有显式规则时

才允许退回通用标题样式，例如：

- `[codex] fix conversation workspace routing and transitions`

## 远端策略

`git-pr` 需要先判断当前账号是否可直接 push `origin`。

如果 `origin` 无权限：

1. push 到可写 fork
2. 从 `fork-owner:branch` 向上游仓库发 PR

更新 fork 上既有分支时，只允许使用 `--force-with-lease`，不允许裸 `--force`。

## 最低验证要求

提交 PR 后至少执行：

- `gh pr view <pr> --json title,url,statusCheckRollup`
- `gh pr checks <pr> --watch`

如果失败项只有 PR 标题格式，应先修标题，再重新确认 checks。

## 当前仓库约定

- 用户说“提交 PR”时，默认使用 `git-pr`
- 不要求用户分别手动调用 `git-commit` 和 `git-pr`
- `git-pr` 对外是单入口，对内才决定是否先补 commit
