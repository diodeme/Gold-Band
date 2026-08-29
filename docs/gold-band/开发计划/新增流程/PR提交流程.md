# Issue 与 PR 提交流程

## 目标

为项目提供两个共享 GitHub 协作 SKILL：

- `git-issue` 负责 Issue 取证、分类、去重、模板填充、用户审阅、发布和回读验证。
- `git-pr` 负责 PR 发布全流程，并在需要时复用 `git-commit` 完成本地提交。

两个 SKILL 对 Claude 与读取 `.agents` 约定的 Agent 共享，但只维护一份正文。GitHub 原生模板是内容 schema 真源，SKILL 只维护工作流和安全边界。

## 设计判断

现有“commit 与 PR 分层”的方向正确，缺口是 PR SKILL 尚未落地、Issue 没有统一流程、跨 Agent 目录没有唯一真源。这属于正确设计但实现不完整，不需要复制 commit 规则或建立新的 GitHub 客户端。

实现优先复用：

- 官方 GitHub CLI `gh`；
- GitHub Issue Forms 与 PR template；
- 仓库实时 labels、rulesets、default branch 与 checks；
- 现有 `git-commit` SKILL；
- Conventional Commits 与 semantic PR title workflow。

## 文件布局与身份

```text
.claude/skills/
├── git-issue/
│   ├── SKILL.md
│   └── agents/openai.yaml
└── git-pr/
    ├── SKILL.md
    └── agents/openai.yaml

.agents/
└── skills -> ../.claude/skills

.github/
├── ISSUE_TEMPLATE/
│   ├── config.yml
│   ├── bug-report.yml
│   ├── feature-request.yml
│   ├── performance-issue.yml
│   └── technical-proposal.yml
└── PULL_REQUEST_TEMPLATE.md
```

`.claude/skills` 是 canonical directory；`.agents/skills` 是兼容投影，不拥有独立生命周期。禁止用目录复制、双写或两份模板兼容软链接失败。本次协作 SKILL 提交只维护 canonical 内容；环境级链接由 workspace 管理者单独设置。

## 强制审阅门禁

Issue 和 PR 的外部写入统一使用如下状态转换：

```text
collect evidence -> prepare preview -> awaiting approval -> approved revision -> publish -> verify
                                    \-> revise -> awaiting approval
```

约束：

1. 用户最初要求“提交 Issue/PR”只授权生成完整预览，不能批准尚未展示的内容。
2. 预览必须展示仓库、完整标题、完整正文和全部元数据。
3. Agent 必须等待用户在后续消息中明确批准。
4. 批准只绑定当前预览 revision；标题、正文、metadata、PR diff、commit set、base/head 或 remote 变化后必须重新审阅。
5. Issue 批准前禁止 `gh issue create/edit/comment`；PR 批准前禁止 `git push` 与 `gh pr create/edit/ready/comment`。
6. 发布后只允许回读验证；继续编辑、评论、推送或改为 ready 都需要新的授权和相应预览。

## Issue 工作流

1. 从 Git remote 解析并通过 `gh repo view` 确认目标仓库、权限和 Issues capability。
2. 读取 Issue Forms，从 Bug、Feature、Performance、Technical Proposal 中选择最接近的 schema。
3. 使用特征词搜索 open/closed issues；疑似重复时先让用户决定停止、补充既有 Issue 或创建不同 Issue。
4. 动态读取 labels，只能选择现存 label；assignee、milestone、project 等 metadata 必须显式请求并进入预览。
5. 默认把中文需求整理为英文发布内容，保留代码、日志、命令、identifier 和 quoted error 原文；用户明确要求中文时才使用中文。
6. Bug 根因必须标记 `Verified / Hypothesis / Unknown`，并回溯设计意图和缺陷形成路径；不能把推测写成事实。
7. 发布前清理 credential、个人数据、私有 URL 和无必要的本机绝对路径。
8. 获批后创建或更新 Issue，再通过 `gh issue view` 回读 number、title、body、labels、state 和 URL。

## Issue Forms

四类模板边界：

- Bug：实际/预期、最小复现、环境、证据、设计意图、根因状态、影响和回归验收。
- Feature：用户问题、使用场景、目标/非目标、现有组件和行业方案评估、期望结果和验收。
- Performance：数据规模、基线、目标、测量方法、瓶颈状态、正确性约束和性能验收。
- Technical Proposal：权威事实源、不变量缺口、现有方案、设计方向、迁移删除、性能/过度设计影响和接口验收。

模板统一使用英文。`config.yml` 保留 blank issue，用于文档、咨询或无法归入四类的事项。安全漏洞、credential 或隐私数据不得作为公开 Issue 发布。

## PR 工作流

1. 动态解析 repo、default branch、current branch、remotes、viewer permission 和 existing PR。
2. 检查 worktree、staged diff、commits 与 `base...HEAD`；禁止从默认分支直接发布。
3. 未提交改动交给 `git-commit` 处理，不复制其 staging、message 和 commit 安全规则。
4. 确认代码修改已同步产品设计与开发计划，并且验证证据属于最终 commit set。
5. 读取 PR template、semantic workflow、ruleset 和 checks；生成符合当前规则的 Conventional PR title。
6. 同一 head branch 已有开放 PR 时准备更新，不重复创建。
7. 普通 push 优先；origin 不可写时先查找既有 fork。禁止裸 `--force`，改写历史必须另行授权且只能使用 `--force-with-lease`。
8. 预览包括 repo、base/head、draft、title、完整 body、metadata、linked issues、commits 和 push remote。
9. 获批后 push 并创建或更新 PR，再通过 `gh pr view` 回读并观察 checks，至少确认 semantic title 结果。
10. 只有 title check 失败时，也必须先生成修正后的完整预览并重新获批；其他 CI 失败如实报告，不扩大为自动修复任务。
11. 不自动 merge、auto-merge、delete branch 或把 draft 改为 ready。

## PR 模板

PR template 统一包含以下可裁剪章节：

- Summary
- Root Cause / Design Rationale
- Changes
- Verification
- Documentation
- Performance and Overdesign Review
- Risks / Limitations
- Related Issues

不适用的可选章节可以删除，不能堆叠空标题或 `N/A`。只有合并能够完整解决 Issue 时使用 `Closes #N`，否则使用 `Refs #N`。

## 验收计划

- 使用 `skill-creator/scripts/quick_validate.py` 分别验证两个 SKILL。
- 契约测试确认 frontmatter 只有 `name` 和 `description`，OpenAI metadata 引用正确的 `$skill-name`。
- 契约测试确认两个 SKILL 都包含“后续消息明确批准”和“preview revision 变化后重新批准”的门禁。
- 契约测试确认四类 Issue Forms 为英文、含 Acceptance criteria，PR template 含验证、文档和方案自评审章节。
- `npm run test:collaboration-skills` 纳入 `.github/workflows/pr-checks.yml`。
- workspace 管理者设置 `.agents/skills` 后，应单独确认其为指向 `../.claude/skills` 的真实相对符号链接，并验证两个发现入口读取内容一致；该环境操作不阻塞 canonical SKILL 内容提交。

## 方案自评审

### 过度设计

方案不新增运行服务、缓存、队列、数据库、GitHub SDK 或自研模板渲染器。四类 Issue Form 对应不同证据模型，不能由一个巨型模板稳定表达；其余事项继续使用 blank issue。SKILL 只保存流程，不复制内容 schema。

### 性能影响

协作 SKILL 仅在人工触发时运行。Issue 去重搜索必须限制结果数量并使用特征词收敛；PR 先读取 status、stat、name-only 和 commit metadata，只在需要时检查具体 diff，避免无界历史和全仓库内容加载。符号链接不复制磁盘内容。无产品运行时、渲染、内存、队列或锁影响，不需要 benchmark。

## 实施状态

- [x] 初始化 `git-issue` 与 `git-pr` 标准 SKILL 结构。
- [x] 实现绑定 preview revision 的强制用户审阅门禁。
- [x] 新增四类英文 Issue Forms 与英文 PR template。
- [x] 新增协作 SKILL 与模板的 canonical 内容契约测试并接入 PR checks。
- [ ] workspace 管理者单独创建并验证 `.agents/skills -> ../.claude/skills` 真实符号链接。
- [x] 完成 SKILL validator、canonical 内容契约测试和最终差异验收。
