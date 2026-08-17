# 右侧源码管理与 GitHub 集成技术方案

## 0. 文档状态

- 状态：已完成（本地 Git 读取/写入、可取消远端与 stash 操作、GitHub capability/login、PR/Issue 查询、PR diff、Commit 主从审阅、提交归属、跨文件 Diff 导航、会话缓存、Git 状态监控与 operation event subscription 均已完成）
- 日期：2026-08-12
- 范围：Gold Band 桌面端右侧源码管理、常见 Git 操作、Commit Patch 审阅、GitHub PR/Issue 集成
- Git 执行基线：系统 Git CLI + Rust typed service
- GitHub 执行基线：系统 `gh` CLI，不捆绑、不自动安装、不由 Gold Band 保存 token
- UI 基线：现有 Right Workspace、文件工作区、CodeMirror/Atomic、shadcn/ui、Tailwind CSS
- 关联基线：`AI-DYNAMIC工作空间树与Git基础设施V2技术方案.md` 中已实现的 Git capability、checkpoint、worktree 与 runtime workspace 语义

## 1. 结论

Gold Band 不需要重新选择 Git 技术栈。现有 `src/git.rs` 使用系统 Git CLI，并通过 Rust 类型化服务管理 capability、checkpoint 和 worktree，这一方向正确，能够复用用户机器上的：

- Git config 与 credential helper
- Git Credential Manager
- SSH、SSH agent 与 askpass
- attributes、filters、LFS、hooks
- submodule 与 worktree 原生语义

当前问题不是根本性的底层设计缺陷，而是既有设计只完成了 runtime 所需的子集，尚未形成面向用户的完整源码管理领域。应继续扩展现有服务，并补齐：

1. 稳定机器格式解析。
2. Repository/workspace 统一锁。
3. status、diff、index、refs、history、stash、remote 等用户能力。
4. 可取消的长任务模型。
5. 右侧源码管理资源。
6. 基于系统 `gh` 的 GitHub capability gate 与 PR/Issue 功能。

不得为了快速增加按钮而在 Tauri command 或前端中散落 `git`/`gh` 命令，也不得引入 libgit2、git2-rs、gix 或 JavaScript Git 实现作为第二套写入语义。

## 2. 目标与首版边界

### 2.1 本地 Git 首版

首版包含：

- Git capability、repository、HEAD、当前分支和 upstream 状态。
- staged、unstaged、untracked、conflict 状态与文件统计。
- 文件级和全部 stage/unstage。
- staged-only commit，支持提交主题和正文。
- 本地/远程分支查看。
- 分支创建、切换、重命名和仅安全删除已合并本地分支。
- 本地 tag 查看、annotated/lightweight tag 创建、本地删除和显式 push tag。
- worktree 查看和创建。
- fetch、pull、push。
- stash create 与 stash apply。
- Commit 列表、标准桌面多选、IDEA 式按文件演化链聚合的最终 Diff、提交归属和跨文件 Diff 审阅。

首版不包含：

- discard/reset。
- force push。
- amend、cherry-pick、交互式 rebase。
- stash pop/drop。
- worktree 删除。
- 远程分支和远程 tag 删除。
- 内置 merge/rebase 冲突编辑器。

pull 默认使用 `ff-only`。用户可显式选择 merge 或 rebase；若产生冲突，Gold Band 展示冲突状态并允许 abort，但不自动改写用户文件。

### 2.2 GitHub 首版

首版包含：

- `gh` 安装、登录和 repository mapping 探测。
- PR 列表、筛选、详情、checks/review 状态、文件列表、diff、打开网页。
- PR 创建，支持 draft/ready、base/head、title/body。
- Issue 列表、状态/标签筛选、搜索、详情和打开网页。

首版不包含：

- PR merge、review、comment、edit、close/reopen。
- Issue create、edit、comment、close/reopen。
- Gold Band 自己管理 GitHub OAuth token。

## 3. 既有实现调研结论

### 3.1 可借鉴部分

- staged/unstaged 分区符合常见 Git 客户端心智。
- Git 操作集中在后端 service，而不是前端自行执行命令。
- 变更文件可以在同一工作空间中直接查看 diff。

### 3.2 不应复用部分

历史实现基于 `git status --porcelain` 的字符串切片，不能完整处理：

- rename/copy 的双路径语义。
- 文件名空格、引号、换行和特殊字符。
- unmerged entries。
- submodule 状态。
- branch/upstream/ahead/behind。

它还缺少 repository/workspace 锁、长任务取消、结构化错误、worktree 归属和 runtime 协调。因此 Gold Band 只参考产品分区，不直接移植其服务或 UI 代码。

## 4. 现成组件与复用决策

| 领域 | 方案 | 决策 |
|---|---|---|
| Git 执行 | 系统 Git CLI | 继续采用，是唯一写入后端 |
| Git 进程 | `process::background_command()` | 所有非交互 Git/gh 调用必须使用 |
| 长进程 | `ManagedProcessGroup` | fetch/pull/push、gh login、PR create 使用，支持进程树取消 |
| 完整文件浏览 | `FileWorkspacePanel + WorkspaceFileTree + fileExplorerStore + fileContentStore` | 直接复用，不新增第二套文件树 |
| Git 变更列表 | shadcn/ui + Tailwind | 只实现 Git 领域的紧凑分组列表，不承担完整文件浏览 |
| 普通文件查看/编辑 | `WorkspaceFileEditor` | 直接复用现有 `file` resource |
| Markdown 文档 | `WorkspaceFileEditor` + CodeMirror + Atomic 低层扩展 | PR/Issue 查看和 PR body 编辑统一复用 |
| 文件 Diff | CodeMirror `unifiedMergeView` | 从现有 turn diff 提取通用 viewer |
| Commit 历史 | shadcn Context Menu + `react-resizable-panels` | 响应式主从列表；复用成熟菜单和 split 组件，不保留 Git Graph |
| GitHub | 系统 `gh` CLI JSON 输出 | 采用，不引入 Octokit，不读取 token |
| 消息 Markdown | prompt-kit/Streamdown | 仅用于聊天和流式消息，不用于 PR/Issue 文档主体 |

### 4.1 Commit 历史交互

Git Graph 和关系分析不再作为首版能力。真实仓库的多 ref DAG 在窄右栏中占用空间、拖动成本高，不能直接帮助用户完成选 Commit、看文件和审阅 Diff；“经历过哪些分支”也不是 Commit 对象可恢复的事实。旧 renderer、adapter、类型、API、测试和 `@tomplum/react-git-log` 依赖全部删除，不保留隐藏入口或兼容层。

新历史区复用 `react-resizable-panels` 和 shadcn Context Menu/Dialog：

- 无选择时为 Commit 单栏；选择后宽屏为左 Commit 列表、右聚合文件列表，窄屏离散退化为“提交/更改”单栏。
- 单击单选，`Shift` 范围，`Ctrl/Cmd` 增减，`Ctrl/Cmd + Shift` 合并范围；选择以当前可见稳定 OID 数组计算，不使用 DOM index 作为身份。
- 右键未选中的 Commit 时先收敛为单选；右键已选中的 Commit 保留当前 selection。
- 右键菜单提供 8 位/完整 SHA 和提交归属；归属只展示当前 contains refs、相对目标分支路径、first merge 与父提交。
- 长列表行使用 `content-visibility: auto` 限制布局开销，选择和 hover 不进入全局 Context。

## 5. 总体架构

源码管理首次加载复用已有 `GitRepositoryService.probe()`、`get_git_capability` 与 `initialize_git_repository`，建立 capability gate，而不是直接把完整 snapshot 的任意失败投影成一个 error：

- `not-installed`：不请求 snapshot/history，显示 Git 安装引导和重新检测。
- `repository-required`：显示非 Git 仓库状态，typed command 执行 `git init`；不 stage、不 commit。
- `head-required`：允许读取源码管理 snapshot；history 对 unborn HEAD 返回稳定空页，用户从“更改”完成首次提交。
- `worktree-required / repository-unavailable`：按稳定 capability 状态展示针对性恢复建议。
- `ready`：snapshot/history 并行加载，真实命令失败才进入结构化错误态。

capability 仅在首次进入、不可用状态的显式重试或初始化终态读取；已 ready 的后台 watcher refresh 不重复 probe。Tauri capability/init command 进入 blocking pool，避免 Git 进程等待占用 IPC event loop。

```text
Right Workspace / Source Control
  -> RuntimeApi typed commands + events
  -> Tauri command/view model
  -> GitCoordinationService
       -> GitRepositoryService
       -> GitIndexService
       -> GitHistoryService
       -> GitRefService
       -> GitRemoteService
       -> GitWorkspaceManager
       -> GitHubCliService
  -> GitCommandRunner / GhCommandRunner
  -> process::background_command()
  -> ManagedProcessGroup for long operations
```

### 5.1 Repository 与 workspace 身份

Repository identity 使用 Git common dir，而不是传入目录字符串：

```rust
pub struct GitRepositoryIdentity {
    pub repo_root: Utf8PathBuf,
    pub common_dir: Utf8PathBuf,
}
```

Workspace identity 使用：

- repository common dir
- canonical worktree path
- branch/HEAD
- ownership：`user | runtime`
- runtime status：`active | frozen | merging | merged | released`

这样同一个 repository 的 main 与多个 worktree 能共享 repository lock，同时保留独立 workspace lock。

### 5.2 唯一协调层

新增 `GitCoordinationService`，runtime checkpoint/worktree 与用户 Git UI 必须共同使用。

不得只给 UI 操作加锁而让 orchestrator 绕过，否则仍可能出现：

- Agent 写文件时 UI commit。
- runtime checkpoint 时 UI stage/unstage。
- worktree add/remove 与 fetch/ref 更新并发。
- frozen runtime workspace 被用户切换分支或 pull。

## 6. 数据结构

### 6.1 Repository 快照

```ts
interface GitRepositorySnapshotVm {
  projectId: string;
  repoRoot: string;
  commonDir: string;
  workspacePath: string;
  headOid: string | null;
  currentBranch: string | null;
  detached: boolean;
  unborn: boolean;
  upstream: GitUpstreamVm | null;
  remotes: GitRemoteVm[];
  lock: GitLockVm;
  revision: string;
}
```

`revision` 是 opaque token，由 HEAD、index、refs 与 workspace state 共同派生。前端只比较是否变化，不解析内容。

### 6.2 工作区状态

```ts
interface GitWorkspaceStatusVm {
  snapshotRevision: string;
  branch: GitBranchStatusVm;
  conflicts: GitFileChangeVm[];
  staged: GitFileChangeVm[];
  unstaged: GitFileChangeVm[];
  untracked: GitFileChangeVm[];
  operationInProgress: GitInProgressOperationVm | null;
}

interface GitFileChangeVm {
  path: string;
  oldPath?: string | null;
  kind: 'added' | 'modified' | 'deleted' | 'renamed' | 'copied' | 'type-changed' | 'unmerged' | 'untracked';
  indexStatus: string | null;
  worktreeStatus: string | null;
  binary: boolean;
  submodule: boolean;
  addedLines: number | null;
  deletedLines: number | null;
}
```

### 6.3 Refs 与 worktree

```ts
interface GitRefVm {
  fullName: string;
  shortName: string;
  kind: 'local-branch' | 'remote-branch' | 'tag';
  targetOid: string;
  peeledOid?: string | null;
  upstream?: string | null;
  ahead?: number | null;
  behind?: number | null;
  checkedOutWorktreePaths: string[];
}

interface GitWorktreeVm {
  path: string;
  headOid: string;
  branch: string | null;
  main: boolean;
  detached: boolean;
  locked: boolean;
  lockReason?: string | null;
  prunable: boolean;
  ownership: 'user' | 'runtime';
  runtimeStatus?: string | null;
}
```

### 6.4 Commit 审阅与归属

```ts
interface GitCommitVm {
  oid: string;
  parentOids: string[];
  subject: string;
  body: string;
  author: GitSignatureVm;
  committer: GitSignatureVm;
  refs: GitRefLabelVm[];
  sourceRef?: string | null;
  runtimeCheckpoint: boolean;
}

interface GitCommitReviewVm {
  selectedOids: string[];
  revision: string;
  commits: Array<{
    commit: GitCommitVm;
    beforeOid: string | null;
    files: GitCommitFileChangeVm[];
  }>;
  files: Array<{ path: string; oldPath?: string; beforeOid?: string; beforePath?: string; afterOid: string }>;
  totals: { commitCount: number; fileCount: number };
}

interface GitCommitReachabilityVm {
  oid: string;
  containingRefs: GitRefLabelVm[];
  targetRef: string;
  targetOid: string;
  targetPath: 'tip' | 'direct' | 'merged' | 'not-contained';
  firstMergeOid: string | null;
  parentOids: string[];
}
```

### 6.5 Stash

```ts
interface GitStashEntryVm {
  refName: string;
  oid: string;
  baseOid: string;
  message: string;
  author: GitSignatureVm;
  createdAt: string;
}
```

### 6.6 长操作

```ts
interface GitOperationVm {
  operationId: string;
  kind: GitOperationKind;
  repositoryCommonDir: string;
  workspacePath?: string | null;
  status: 'queued' | 'running' | 'succeeded' | 'failed' | 'cancelled' | 'conflicted';
  cancelable: boolean;
  startedAt: string | null;
  completedAt: string | null;
  error?: AppErrorVm | null;
}
```

## 7. Runtime API 与 Tauri 接口

### 7.1 查询接口

```ts
getSourceControlSnapshot(input): Promise<SourceControlSnapshotVm>
getGitDiff(input: GitDiffInput): Promise<FileComparisonVm>
getGitHistory(input: GitHistoryQueryVm): Promise<GitHistoryPageVm>
getGitCommitReview(input): Promise<GitCommitReviewVm>
getGitCommitReachability(input): Promise<GitCommitReachabilityVm>
listGitStashes(input): Promise<GitStashEntryVm[]>
```

### 7.2 短操作接口

```ts
executeGitMutation(input: GitMutationInput): Promise<GitMutationResultVm>
```

```ts
type GitMutationResultVm =
  | {
      scope: 'workspace';
      status: GitWorkspaceStatusVm;
      repositoryRevision: string;
    }
  | { scope: 'repository' };
```

`workspace` 结果用于只改变 index/worktree status 的 Stage/Unstage，前端局部合并；`repository` 表示 refs 或 repository 结构可能变化，前端并行刷新 snapshot/history。返回作用域是 mutation 领域契约，不由页面根据按钮名称猜测。

`GitMutationInput` 是明确 tagged union，只允许：

- `stage-paths`
- `stage-all`
- `unstage-paths`
- `unstage-all`
- `commit`
- `branch-create`
- `branch-switch`
- `branch-rename`
- `branch-delete-safe`
- `tag-create`
- `tag-delete-local`
- `worktree-create`

前端不得传入任意 Git args。

### 7.3 长操作接口

```ts
startGitOperation(input: GitOperationInput): Promise<GitOperationVm>
getGitOperation(operationId: string): Promise<GitOperationVm>
cancelGitOperation(operationId: string): Promise<GitOperationVm>
subscribeGitOperationUpdates(listener): Promise<Unlisten>
subscribeGitHubOperationUpdates(listener): Promise<Unlisten>
```

长操作类型：

- fetch
- pull
- push
- push-tag
- stash-create
- stash-apply
- github-login
- github-pr-create

本地 Git 与 GitHub operation 使用各自的 typed view model 和事件名，共享“先订阅、再启动、按 operationId 合并早到事件”的前端生命周期，不合并成可传任意参数的通用命令通道。

## 8. Git CLI 契约

### 8.1 命令安全

- 所有命令使用 `process::background_command()`。
- 禁止通过 cmd、PowerShell、bash 或字符串拼接执行。
- cwd 和 args 逐项传入。
- 路径前使用 `--` 或 `--pathspec-from-file=- --pathspec-file-nul`。
- 多路径通过 stdin NUL 分隔传入。
- commit/PR body 通过 stdin 或 `--file=-`/`--body-file=-` 传入，避免命令行泄露与长度限制。
- stdout/stderr 设置上限；超限时保留裁剪摘要，不无限积累内存。

### 8.2 机器格式

优先使用：

```text
git status --porcelain=v2 -z --branch --untracked-files=all
git diff --raw -z
git diff --numstat -z
git for-each-ref --format=<explicit fields>
git worktree list --porcelain
git log --topo-order --parents --decorate=full --source --format=<NUL fields>
git push --porcelain
git stash list --format=<explicit fields>
```

不得依赖本地化的人类可读文本判断业务状态。

### 8.3 Git status 解析

完整支持 porcelain v2：

- ordinary changed entry `1`
- rename/copy entry `2` 及后续原路径
- unmerged entry `u`
- untracked `?`
- ignored `!` 仅在显式请求时处理
- branch oid/head/upstream/ahead-behind headers

所有 path 都按字节边界/NUL 解析，不按空格切分。

### 8.4 Diff 语义

- staged：HEAD 与 index。
- unstaged：index 与 worktree。
- untracked：空内容与 worktree 文件。
- commit：两个 tree/commit。
- PR：GitHub PR base/head 或 `gh pr diff` 结果转换后的统一 comparison。

工作区 status 在 porcelain v2 之后并行执行 staged、unstaged 各一次批量 `git diff --numstat -z --no-ext-diff --no-textconv -M -C`，按路径合并 tracked 文件统计；Git 命令数固定为 2，不允许退化为逐文件 N+1。未跟踪文件在进入 index 前不读取正文计算行数，避免 watcher 刷新触发无界文件 I/O。只需要 branch/revision/冲突判断的内部调用使用轻量 status，不附带 numstat 查询。

所有文本 comparison 在限制判断和 UTF-8 校验后统一规范换行符：CRLF 和单独 CR 均转换为 LF，统计与返回给 CodeMirror 的 before/after 共用规范化内容。CodeMirror `collapseUnchanged` 默认折叠未变化内容，仅展示差异与上下文；不能因工作树与 Git blob 的换行风格不同产生全文件 Diff 或错误 summary。

文本内容最终转成统一 `FileComparisonVm`：

```ts
interface FileComparisonVm {
  path: string;
  before: FileTextVersionVm | null;
  after: FileTextVersionVm | null;
  stats: FileDiffStatsVm;
  limitationCode?: string | null;
}
```

二进制、超限、submodule 和无法读取内容使用 limitation code，不向 CodeMirror 传入伪文本。

### 8.5 Commit

- 只提交 staged 内容。
- 没有 staged change 时禁用。
- 不使用 `--no-verify`，正常执行用户 hooks。
- 不覆盖用户签名/GPG 配置。
- runtime checkpoint 继续使用内部专用 author/trailer 和 `--no-verify`，不能与用户 commit 混用。

### 8.6 Branch

- 切换使用 `git switch`。
- dirty workspace 不自动 stash。
- 分支安全删除只允许 `git branch -d`，不提供 `-D`。
- 被其他 worktree checkout 的分支禁止切换/删除，并返回占用路径。
- runtime branch 默认隐藏在常用列表，可显式展开；禁止默认 push。

### 8.7 Tag

- 默认创建 annotated tag。
- 用户可显式选择 lightweight。
- 删除仅作用于本地 tag，并二次确认。
- push tag 是独立显式操作。
- 首版不删除远程 tag。

### 8.8 Worktree

新建 worktree 支持：

- 从选定 ref/commit 创建新分支并 checkout。
- checkout 尚未被其他 worktree 占用的现有本地分支。
- 默认路径位于 repository 同级目录，格式为 `<repo-name>-<branch-slug>`，用户可编辑。

禁止：

- 目标路径已存在。
- 路径与任一已登记 worktree 冲突。
- checkout 已被其他 worktree 占用的分支。
- 对 runtime-owned worktree 使用用户创建流程覆盖。

Worktree 还支持 Git 原生安全删除：

- 前端仅提交 typed `worktree-remove { path }`，后端先将路径规范化并与 `git worktree list` 的权威记录精确匹配。
- 当前 command 所在 worktree 禁止删除；不存在或已经变化的目标返回稳定错误码。
- 删除使用 repository write lock 和 `git worktree remove`，不使用 `--force`，不调用文件系统递归删除，也不顺带删除关联 branch。
- dirty/untracked worktree 由 Git 拒绝，映射为 `git.worktree-remove-dirty` 并保留脱敏后的原始原因。
- UI 使用行级菜单与确认 Dialog，展示完整目标路径；pending action 保存目标 path，仅目标行显示 spinner。
- 仓库四个列表的容器、滚动区与行建立完整 `min-width: 0 / overflow-hidden` 约束链，主文案和辅助 path/ref 分配有界弹性空间，操作区固定，长 Stash/Worktree 文本不得撑宽右侧客户端。

Merge/Rebase 状态机的 marker 语义不同：`MERGE_HEAD` 可判定 Merge 进行中，Rebase 进行中只以 `rebase-merge` / `rebase-apply` 目录为准；`REBASE_HEAD` 仅提供当前重放 Commit 的 OID/subject，因为 Git 成功结束后仍可能保留该文件。watcher 继续监听全部 marker 以触发刷新，但 snapshot 不得把事件触发源误当成生命周期事实。

### 8.9 Fetch/Pull/Push

- fetch/pull/push 使用长操作模型。
- 默认 `GIT_TERMINAL_PROMPT=0`，禁止隐藏终端等待输入。
- 允许用户现有 GUI credential helper、GCM、SSH agent/askpass 正常工作。
- pull 默认 `ff-only`。
- merge/rebase 必须用户显式选择。
- push 不提供 force；首次 push 可显式勾选 set-upstream。
- non-fast-forward 返回结构化错误，不自动 pull/rebase。
- 用户选择的 remote 以规范化 repository common-dir 为作用域写入带版本号、有容量上限的用户偏好；默认值依次取仍有效的偏好、upstream remote、第一项 remote，禁止每次打开对话框重置为第一项。

### 8.10 Stash

stash create：

- message 可选。
- 默认不包含 untracked。
- 可显式选择 include-untracked。
- 不提供 include-ignored。

stash apply：

- 选择明确的 stash ref。
- 默认使用 apply，不删除 stash。
- 可显式选择恢复 index 状态。
- 冲突时进入 `conflicted`，保留工作区现状并展示冲突文件。
- 不自动 reset，不自动 drop。

## 9. Repository/Workspace 锁

### 9.1 Repository 写锁

以下操作获得 common-dir 级写锁：

- worktree add
- branch/tag/ref 创建或删除
- fetch
- push tag
- runtime checkpoint ref 更新

### 9.2 Workspace 写锁

以下操作获得 canonical-worktree-path 级写锁：

- Agent 可写执行
- checkpoint
- stage/unstage
- commit
- branch switch/rename
- stash create/apply
- pull/merge/rebase/abort

### 9.3 UI 权限

runtime 正在持有 workspace writer lock 时：

- 允许查看 status、diff、history。
- 禁止 stage、unstage、commit、stash、pull 和切换分支。

runtime workspace 为 frozen 时：

- 允许查看。
- 禁止改变 ancestry 的操作。

runtime 内部分支 push 默认禁用，避免发布 `gb-dyn/*` 等内部 refs。

## 10. Git 状态刷新

新增 repository-scoped `GitStateMonitor`：

- 复用现有 workspace 文件 watcher 的普通文件事件。
- 额外监听通过 `git rev-parse --git-path` 解析出的 HEAD、index、refs、packed-refs 等 Git 元数据路径。
- 事件合并后 debounce，再重新读取 canonical status。
- Git/gh 操作成功后立即刷新，不等待 watcher。
- 源码管理资源可见时允许低频 fallback poll；隐藏或无订阅时停止。
- 一个 repository/workspace 只存在一个 monitor，多 UI 消费者共享快照。

不得让每个文件行、每个 tab 或每个 React hook 独立轮询 Git。

### 10.1 前端会话缓存与失效

源码管理不能把 repository snapshot、history 和导航状态保存在 `SourceControlWorkspacePanel` 的组件本地。右侧 Dock 只挂载 active resource，打开 `file-diff` 会卸载源码管理面板；若状态跟随组件生命周期，用户返回时会重复读取 Git，并丢失当前分区、分页、选择和详情。

采用独立 `SourceControlStore`，以 `projectId + canonical workspacePath` 作为会话身份；首次请求前以规范化 requested path 建立路由别名，snapshot 返回后注册 canonical path 别名。每个会话统一保存：

- repository snapshot、history pages、加载状态和完整结构化错误 `code + params`；Git operation 终态刷新后仍保留原始失败原因。
- 当前源码管理分区、history page、selected OID Set 和 focused commit。
- commit detail、relations detail、详情加载状态和请求 revision。
- commit subject/body 草稿、结构化 pending action `kind + path`、当前 Git operation 及事件订阅状态。

失效规则：

- 首次无缓存时加载，普通 Right Workspace Tab 切换、打开 Diff 和返回不失效。
- 已加载源码管理页面依赖 workspace/Git metadata watcher 自动刷新 snapshot/history，不显示普通本地刷新按钮；首次加载失败仍允许重试。Fetch 是显式远程同步命令，不等价于本地刷新。
- Stage/Unstage 成功后使用 `scope=workspace` 结果只合并最新 status 与 repository revision，不刷新未变化的 refs/worktree/stash/remotes/history。
- Commit、branch、tag、worktree 等 refs/结构变化 mutation 返回 `scope=repository`，前端在命令成功后并行读取 snapshot/history，禁止先等完整 snapshot 再串行读取 history。
- pending action 生命周期内禁用同 workspace 的其他 Git 写操作；单文件 Stage/Unstage 仅在目标行按钮显示旋转状态，其他文件行操作按钮不渲染。后台 watcher 刷新使用独立 `refreshing` 状态，不禁用 commit subject/body 或文件操作。Commit/Fetch/Pull/Push 在主操作按钮显示旋转状态。
- 长操作结束后立即刷新 snapshot/history。
- `GitStateMonitor` 事件使对应 repository/workspace 会话失效，不允许由每个组件自行轮询。
- snapshot/history/detail 分别维护请求 revision；旧请求完成后不得覆盖较新刷新或操作结果。
- 缓存最多保留 24 个非活跃会话，使用 LRU 淘汰；有订阅或正在执行 Git 操作的会话不可淘汰。

### 10.2 GitHub 查询缓存与 PR Diff identity

GitHub 数据不得保存在 `SourceControlGitHubView` 的组件本地生命周期中。使用独立 `GitHubDataStore`：

- repository/workspace 会话 key 为规范化的 `projectId + commonDir + workspacePath`，最多 24 项。
- capability、PR/Issue query、PR/Issue detail 分层缓存；同 key 的 in-flight Promise 复用，避免重复 `gh` 进程。
- PR/Issue query 每会话最多 16 项，详情各最多 48 项；显式刷新只强制当前 query 或 detail。
- capability 仅在首次缺失、用户“重新检测”或登录终态后读取；普通 Tab 往返直接返回缓存。
- PR comparison 使用 `projectId + workspacePath + host + repository + prNumber + baseOid + headOid + path` 的不可变 key，最多 96 项；相同 revision 重开立即返回，head/base 改变后自动隔离。
- repository/workspace 会话保存 `section + listState + committedSearch + selected(kind, number) + detailSection` 的轻量导航 locator；打开 Diff 卸载源码管理 renderer 后，重新激活源码管理必须从 locator 与详情缓存恢复原 PR/Issue 详情及子页。搜索输入草稿、loading、错误和正文仍留在最小消费边界，不写入导航状态。
- 缓存是运行期只读投影，不持久化 GitHub token、正文或认证结果到 localStorage。

`GitHubPullRequestDetailVm` 返回 `baseRefOid/headRefOid`。点击文件时把这两个稳定 revision 写入 typed comparison locator；后端校验 40/64 位十六进制 OID 和 repo-relative path，只并行执行 base/head 两次 raw content 请求。旧的每文件 `gh pr diff --name-only` 与 `gh pr view --json baseRefOid,headRefOid,files` 消费路径删除。

GitHub PR/Issue 列表和详情的宽度链从领域根容器贯穿 Tabs、TabsContent、ScrollArea 到单行，统一使用 `min-w-0 / overflow-hidden / max-w-full` 限制在右侧面板内。标题、账号、head/base 分支、label 与文件路径是可压缩省略列；导航按钮、状态和增删统计为固定列，长远端文本不得改变客户端宽度或生成横向滚动。

## 11. 提交历史与多选关系

### 11.1 历史加载

- 使用 topo order。
- 默认查询当前工作树 `HEAD` 的完整可达历史，不使用 `git log --all` 混入未合并旁支；只有显式 ref 筛选才改用目标 ref。
- 初始加载 300 条。
- 后续按 300 条增量加载。
- 页码只展示当前页，不以已加载页数冒充总页数；`nextCursor` 存在时“较早”保持可用，直到实际 Root Commit 所在末页。
- 每页携带 refs revision；refs 已变化时放弃旧 cursor 并重载。
- 历史搜索可按 hash、subject、author 和 ref 收窄。
- runtime checkpoint 默认压缩，可展开。

### 11.2 多选

提交行使用自定义 shadcn Checkbox 维护选中 OID 集合。

两个提交时展示：

- 相同/祖先/后代/分叉关系。
- merge base。
- ahead/behind commit count。
- 两点文件 diff。

三个及以上提交时展示：

- common merge base。
- 两两祖先关系矩阵。
- 相对目标 ref 的包含状态。
- 每个提交首次通过哪个 merge commit 进入目标 ref。

关系计算使用：

- `git merge-base --is-ancestor`
- `git merge-base --octopus`
- `git rev-list --ancestry-path`
- `git rev-list --first-parent --merges`

squash/rebase 导致原 OID 不属于目标分支时，明确返回 `not-contained-by-original-oid`，不能把 patch 相似误报为真实合并。

## 12. 右侧源码管理 UI

### 12.1 Resource

在 `RightWorkspaceResource` 增加：

```ts
interface SourceControlWorkspaceResource extends RightWorkspaceResourceBase {
  kind: 'source-control';
  projectId: string;
  workspaceId?: string | null;
  workspacePath?: string | null;
}
```

resource key 必须包含 project/workspace 身份，查看 dynamic child 时绑定 child worktree，不能始终指向 main。

### 12.2 内部分区

源码管理包含：

1. 更改
2. 历史
3. 仓库
4. GitHub

使用现有右侧 Dock/Sheet 响应式容器。宽面板可显示主列表与详情双栏；窄面板使用列表/详情单栏切换。

### 12.3 文件工作区复用

完整文件浏览已经由现有文件工作区实现，源码管理不得创建第二套目录树。

- “浏览仓库文件”打开现有 `file-browser` resource。
- 点击普通文件打开现有 `file` resource。
- Git 变更只展示 staged/unstaged/conflict/untracked 的紧凑领域列表。
- Git 列表不拥有文件读取、编辑、保存、目录展开和 watcher 状态。

### 12.4 通用文件比较资源

现有 `TurnFileWorkspaceResource` 与 `TurnFileWorkspacePanel` 需要从 turn-only 模型提升为通用文件比较资源：

```ts
type FileComparisonSource =
  | { kind: 'turn'; locator: TurnFileLocatorVm; changeSetId: string; changeId: string }
  | { kind: 'git-worktree'; projectId: string; workspacePath: string; path: string; area: 'staged' | 'unstaged' }
  | { kind: 'git-commit'; projectId: string; beforeOid: string; afterOid: string; path: string }
  | { kind: 'github-pr'; projectId: string; repository: string; prNumber: number; path: string };
```

所有 source 最终返回统一 `FileComparisonVm`，共同使用：

- CodeMirror `unifiedMergeView`
- 现有主题和语言 extension
- diff chunk 上一处/下一处导航
- 增删统计
- Markdown 新增文件/单版本的 `WorkspaceFileEditor`

不得复制第二套 CodeMirror diff 配置。

### 12.5 更改区

仓库标题右侧使用动态同步按钮：`behind > 0` 时显示 `↓behind ↑ahead` 并执行 Pull，否则显示 `↑ahead` 并执行 Push；同步为零时禁用，未设置 upstream 时允许首次 Push。Fetch 保持独立，用于联网更新 remote refs。Push non-fast-forward 不自动触发 Pull，只展示结构化失败，由用户显式 Fetch、Pull、Push。按钮打开原有 typed 对话框，复用 repository-scoped remote 偏好和统一 operation 状态。更改区工具栏不再重复展示同步文字按钮，仅保留右侧 `…`，承载全部暂存、全部取消暂存和保存为 stash。terminal operation 结果保留在 repository/workspace 会话中，跨 Tab 可见，直到用户关闭或下一次 operation 替换。

Fetch Dialog 的 `prune` 开关默认关闭，UI 使用用户领域文案“移除远端已删除的分支记录”，并明确说明不会删除本地分支或工作区文件；接口仍使用 typed `prune: boolean`，不从文案反推行为。

- 顶部显示当前分支、upstream、ahead/behind 和同步操作。
- 分区顺序：冲突、已暂存、未暂存、未跟踪。
- 每行显示状态、路径、rename old path、增删统计和可用操作。
- tracked 文件增删统计由 staged/unstaged 两次批量 numstat 提供；未跟踪文件在暂存前不显示统计，暂存后由 index 统计。
- commit composer 紧贴面板底部，包含 subject、可展开 body、commit 按钮。
- 没有 staged change、workspace locked 或存在未解决冲突时禁止 commit。
- stash 放在工具栏菜单，不作为文件行操作。
- 工作区无任何变更时，空状态占满工具栏与 commit composer 之间的剩余高度并水平、垂直居中，使用 `muted-foreground` 弱化展示；有文件时才挂载变更列表滚动区。

### 12.6 历史区

- 首次无历史缓存时显示真实请求 loading；普通分区往返直接恢复 `SourceControlStore` 中的历史页、选择和详情，不插入伪 loading，不重新请求。
- 支持 ref、作者、日期、文本筛选。
- 历史内容区达到 `520px` 时使用 Commit 列表/变更文件主从双栏，列表与详情最低宽度分别为 `220px / 280px`；低于阈值使用“提交/更改”单栏切换，不增加嵌套卡片。断点必须根据历史内容区容器实测宽度判断，不能依赖整个窗口断点。
- 单击单选，Shift 范围，Ctrl/Cmd 增减，Ctrl/Cmd+Shift 合并范围；不显示 Checkbox。
- 任意多个 Commit 只收集各自 first-parent Changes，再按旧到新合并同一文件演化链；Root 与空树比较，同一路径只返回一个首尾终态。
- 右键提供短/完整 SHA 和当前可验证的提交归属。
- 更改、历史聚合、PR 文件统一进入有界 Diff review session，同一会话只占用一个 Tab；左右文件导航与上下差异导航均使用 Tooltip，差异导航可跨文件。
- 三个领域复用同一文件行组件：绿色 A / 蓝色 M / 红色 D + 可压缩 path + `+n -n`。历史统计计算聚合链首尾终态，并按相同 before/after 端点分组执行批量 numstat；禁止逐文件读取正文或 N+1 Git 查询。PR 文件状态一次分页批量读取 REST files API，禁止逐文件请求或前端猜测。
- 文件行纯图标操作使用 shadcn Tooltip；已暂存文件的回转图标明确为 Unstage，不与 Discard 共用语义。

### 12.7 仓库区

仓库区包含分支、tags、worktrees、stash 四个同层级二级 Tabs，使用 shadcn Tabs/DropdownMenu/Dialog/Command 等 copy-in 组件。一次只挂载当前领域列表，`repositoryTab` 与源码管理主分区、分页、选择和 commit 草稿一起进入 repository/workspace 会话缓存；普通主 Tab 往返、打开文件或 Diff 后返回不得重置。

### 12.8 Pull 冲突流程

不实现可视化冲突块合并工具。`GitWorkspaceStatus.operationInProgress` 收紧为 typed `merge | rebase | cherry-pick | revert`，Rebase 同时返回当前 `oid + subject`；前端只投影 canonical Git metadata，不根据 stderr 文案猜状态。

- Merge：确认后只暂存 `git diff --name-only --diff-filter=U -z` 返回的路径，再执行 `git merge --continue`；允许 `git merge --abort`。
- Rebase：确认后只暂存当前 unmerged 路径，再执行 `git rebase --continue`；危险菜单提供 `git rebase --skip` 和 `git rebase --abort`。Skip 确认必须展示当前 Commit 短 SHA/标题并说明整个 Commit 的改动不会应用。
- continue/add/原生命令在同一 workspace 写锁内执行，避免其他写操作插入；设置非交互 editor 环境，命令不会弹出终端编辑器。
- watcher 增加 `MERGE_HEAD / REBASE_HEAD / rebase-merge / rebase-apply` 目标，外部 IDE 的 add/continue/skip/abort 与 Gold Band 自身操作使用同一 snapshot 收敛路径，无轮询。
- 冲突期间普通 Commit、同步、Stage/Unstage 和仓库写操作禁用；冲突文件打开现有普通文件编辑 Tab。

## 13. PR/Issue Markdown 与 Atomic 复用

### 13.1 组件边界

PR/Issue 是文档内容，不是聊天消息：

- PR/Issue 详情 body 使用 `WorkspaceFileEditor`，`editable={false}`，默认 live preview。
- PR create body 使用 `WorkspaceFileEditor`，`editable={true}`。
- title、filter 等短字段使用 shadcn Input。
- 列表摘要使用裁剪后的纯文本，不为每行挂载 CodeMirror。
- prompt-kit/Streamdown 只用于聊天消息、流式文本和简单信息正文。

Gold Band 继续使用 `@uiw/react-codemirror` 加 Atomic 的低层 extension，不直接实例化 Atomic React wrapper：

- `highlightMarkdown`
- `atomicMarkdownSyntax`
- `inlinePreview`
- `tables`

这样继续复用现有主题、只读策略、模式切换、同一 EditorView 生命周期和安全图片策略。

### 13.2 GitHub 链接

PR/Issue body 的链接上下文与 workspace 文件不同：

- `#anchor` 留在当前文档。
- 绝对 HTTP(S) 链接使用 Tauri opener。
- repository 相对链接按 GitHub repo/ref 转换为远端 URL。
- Issue/PR shorthand 可转换为对应 GitHub 页面。
- 不把远端相对路径误解析为用户本地 workspace 文件。

远程图片默认显示为可打开链接，不直接加载未知网络资源。若未来允许 GitHub 图片预览，应建设单独的 host allowlist 与显式网络策略，不能绕过现有安全模型。

## 14. GitHub CLI 集成

### 14.1 Capability

```ts
interface GitHubCapabilityVm {
  status:
    | 'not-installed'
    | 'not-authenticated'
    | 'repository-unresolved'
    | 'ready';
  version?: string | null;
  host?: string | null;
  account?: string | null;
  repository?: string | null;
  remote?: string | null;
  defaultBranch?: string | null;
}
```

探测顺序：

1. `gh --version`
2. `gh auth status --json hosts`
3. 当前 branch upstream remote
4. `origin`
5. 唯一 GitHub remote
6. `gh repo view <owner/repo> --json nameWithOwner,defaultBranchRef`

`gh repo view` 的 repository 是 positional argument；该子命令不支持通用 `--repo` flag。命令参数必须由接口测试固定，避免把其他 `gh pr/issue` 子命令支持的 `--repo` 误套到 `gh repo view`。

多 remote 仍有歧义时由用户选择。选择只写 Gold Band 用户级偏好，不调用 `gh repo set-default` 修改仓库配置。

### 14.2 未安装

`gh` 不存在时：

- GitHub 分区保留入口。
- PR/Issue 功能全部不启用。
- 展示“安装 GitHub CLI 以启用 GitHub 功能”。
- 提供“打开安装页面”和“重新检测”。
- 安装页面指向 GitHub CLI 官方页面。
- 不自动下载、不捆绑、不执行包管理器、不展示 terminal 安装命令。
- 不影响任何本地 Git 功能。

### 14.3 未登录

`gh` 已安装但未登录时：

- 展示登录按钮。
- 用户点击后才启动 `gh auth login --web --clipboard`。
- 通过 `ManagedProcessGroup` 管理隐藏进程。
- 浏览器完成授权，UI 展示等待、取消和重新检测。
- Gold Band 不调用 `--show-token`，不读取 token，不把 token 传给前端或日志。

检测在首次进入 GitHub 分区时懒执行。若之前为未安装/未登录，应用重新获得焦点时自动复检一次，同时保留手动重新检测。

ready capability 按 repository/workspace session 缓存；已 ready 时窗口 focus 不复检。GitHub mapping 只读取 status/remotes 所需字段，不调用完整 source-control snapshot。

### 14.4 PR 查询

使用 `gh pr list/view --json <explicit fields>`，只请求 UI 需要的字段。

列表默认：

- 当前 repository。
- open PR。
- 50 条上限。
- 支持 state、author、base、head、label 和文本筛选。

详情包含：

- title/body/author/state/draft。
- base/head。
- mergeable/mergeStateStatus。
- reviewDecision/latestReviews。
- statusCheckRollup。
- additions/deletions/changedFiles。
- changed file list。
- URL。

PR 文件 Diff 使用 typed `GitComparisonSource::GitHubPr`，PR 详情先批量读取权威 files status/previous filename/stats 后构建连续审阅序列：

1. PR 详情接口用 `gh api repos/{owner}/{repo}/pulls/{number}/files --paginate --slurp` 一次批量返回 typed changed file list 与不可变 `baseRefOid/headRefOid`；点击文件时直接写入 review locator，不再重复查询整 PR。
2. `gh api --hostname <host> --method GET --header "Accept: application/vnd.github.raw+json" <contents-endpoint>` 按 base/head OID 并行读取两端文件内容。
3. 转换为现有 `GitFileComparison`，点击 PR 文件后立即打开现有 `file-diff` review resource，由新 Tab 展示 comparison loading，并支持同 PR 上/下文件连续审阅。

PR 列表项点击时先写入 `selected(kind, number)` locator 并显示详情 loading surface，再读取 detail cache/API；请求成功后原位收敛为详情，失败时保留返回入口与结构化错误。comparison 请求不得成为 PR 详情或文件 Tab 导航的前置条件。

前端不能传递任意 `gh` 参数；repository、PR number、path 都通过 tagged union 字段进入后端校验。新增/删除文件通过缺失的一侧表达；二进制、非 UTF-8 和超过 4 MiB 的内容沿用 `git.binary-diff-unsupported`、`git.text-encoding-unsupported`、`git.diff-too-large`。comparison resource key 包含 project、worktree、host、repository、PR number 和 path，避免 linked worktree 或不同 PR 之间错误复用 tab。

### 14.5 PR 创建

创建前检查：

- GitHub capability ready。
- head/base 明确。
- head 不等于 base。
- head branch 存在 commits ahead of base。
- head 已发布；未发布时用户显式确认先 push。
- 同 head/base 是否已有 open PR。

调用必须提供：

- `--repo`
- `--head`
- `--base`
- `--title`
- `--body-file -`
- 可选 `--draft`

不得让 `gh pr create` 进入交互式提问或自行决定 push/fork。

当前实现以独立 typed preflight 返回 mapped remote、ahead 数、head 发布状态和已有 open PR。UI 未发布分支只提供显式 typed push 入口；创建开始前后端重新执行相同预检。`gh pr create` 复用通用 `GitHubOperation`/`ManagedProcessGroup`，支持取消，PR body 由可编辑 `WorkspaceFileEditor` 生成并只经 stdin 传入，成功后返回结构化 `resultUrl`。

### 14.6 Issue 查询

- 当前 repository 的 issue list。
- open/closed/all。
- labels、author、assignee、milestone 和文本搜索。
- Issue detail 使用明确 JSON fields。
- 详情 body 使用只读 Atomic 文档视图。
- 首版不提供写操作。

## 15. 错误与安全

### 15.1 客户端错误协议

客户端继续只消费：

```ts
interface AppErrorVm {
  code: string;
  params: Record<string, unknown>;
}
```

后端不生成对客文案。前端根据 i18n error code 映射中文/英文文案。

Git CLI 失败时，`params` 还必须包含 `exitCode` 和可选 `reason`。`reason` 是 Git 原始 stderr 与 stdout 的数据投影，不是后端对客文案；进入 DTO 前必须去空行、限制为 2000 字符并脱敏 URL userinfo。前端统一保存完整错误对象，在本地化原因/恢复建议下展示 `reason`，长文本安全换行。

### 15.2 Git 错误码

至少包含：

- `git.not-found`
- `git.repository-not-found`
- `git.head-required`
- `git.workspace-locked`
- `git.runtime-workspace-restricted`
- `git.auth-required`
- `git.non-fast-forward`
- `git.authentication-failed`
- `git.permission-denied`
- `git.remote-repository-not-found`
- `git.remote-host-unreachable`
- `git.remote-unreachable`
- `git.remote-rejected`
- `git.merge-conflict`
- `git.rebase-conflict`
- `git.stash-apply-conflict`
- `git.hook-failed`
- `git.ref-changed`
- `git.branch-in-use-by-worktree`
- `git.worktree-create-failed`
- `git.operation-cancelled`

### 15.3 GitHub 错误码

- `github.gh-not-installed`
- `github.not-authenticated`
- `github.repository-unresolved`
- `github.permission-denied`
- `github.rate-limited`
- `github.repository-not-found`
- `github.pr-already-exists`
- `github.pr-create-failed`
- `github.operation-cancelled`

内部诊断可保存 command kind、exit code 和裁剪后的 stdout/stderr summary，但必须：

- 去除 token/credential。
- 不记录完整 PR/Issue body。
- 不记录任意用户文件内容。
- 不把命令参数整体写日志。

## 16. 实施阶段

### 阶段 1：文档、模型与协调层

- 将本方案同步到产品设计文档中的 Source Control 章节。
- 扩展 `src/git.rs` 的 typed service 分层。
- 建立 repository/workspace lock。
- 让 runtime checkpoint/worktree 共同使用协调层。
- 定义 view model、error code 和 RuntimeApi。

### 阶段 2：本地只读 Git

- porcelain v2 parser。
- status/diff/refs/worktree/stash list。
- [x] history page 与 Commit 元数据。
- [x] 批量 first-parent Changes 收集、文件演化链终态聚合和提交归属。
- [x] GitStateMonitor 与前端 snapshot subscription。
- [x] 删除 Git Graph renderer、adapter 和第三方依赖。

### 阶段 3：通用文件比较资源

- 将 turn-only diff resource 提升为通用 comparison source。
- 抽取统一 CodeMirror merge viewer。
- 保持现有 turn diff 行为不变。
- 接入 Git workspace/commit comparison。

### 阶段 4：本地写操作

- [x] stage/unstage/commit。
- [x] branch/tag/worktree create，以及 branch rename/safe-delete、tag local delete/push。
- [x] stash create/apply。
- [x] fetch/pull/push 长操作与取消的后端模型和状态 UI。
- [x] operation event subscription，替换完成状态轮询。
- [x] lock/restriction/error UI 基础状态。
- [x] Git command 失败原因的结构化透传、credential 脱敏和本地化恢复建议。
- [x] Fetch/Push/Push Tag remote 的 repository-scoped 持久偏好与合法性回退。

### 阶段 5：右侧源码管理

- [x] 新增 `source-control` resource 与入口。
- [x] 更改、历史、仓库分区。
- [x] 复用现有 file/file-browser resource。
- [x] 历史主从双栏、窄栏切换、标准桌面多选、右键菜单和 i18n 契约。
- [x] 所选 Commit 的按文件终态 Diff 聚合与跨文件 Diff 审阅导航。
- [x] repository/workspace-scoped 源码管理会话缓存、请求去重和 stale response 防护。

### 阶段 6：GitHub

- [x] capability gate。
- [x] 未安装/未登录/repository unresolved UI。
- [x] PR list/detail/create。
- [x] PR diff 与通用 comparison viewer 接入。
- [x] Issue list/search/detail。
- [x] PR/Issue Atomic 文档视图与 PR body 编辑。

## 17. 测试计划

### 17.1 Rust Git 接口测试

使用临时真实 Git repository，覆盖：

- Git 未安装/非 repository/unborn HEAD/detached HEAD；unborn 真实仓库必须同时通过 snapshot 与空 history 接口，且保留未跟踪文件。
- staged/unstaged/untracked/conflict。
- staged/unstaged 批量 numstat，包含同一文件 index/worktree 分层统计、rename 和 binary。
- CRLF/LF 与单独 CR 统一为 LF；8000 行文件仅换行风格不同且新增 2 行时必须为 `+2/-0`，正文不含 CR。
- 空格、Unicode、引号、换行文件名。
- rename/copy/type-change/submodule。
- 初次 commit 与普通 commit hooks。
- branch create/switch/rename/safe-delete。
- 分支被 worktree 占用。
- annotated/lightweight tag。
- worktree create 与路径冲突。
- stash create/apply、include-untracked、apply conflict。
- fetch/pull ff-only/push/upstream/non-fast-forward。
- Git operation 失败终态保留结构化 code、exitCode 和脱敏 reason。
- runtime/UI lock 竞争。

### 17.2 历史测试

- 线性历史。
- 默认 HEAD 历史排除未合并旁支，并跨多页一直到 Root Commit。
- 双分支 merge。
- 多层 merge。
- octopus merge。
- merge-base、ancestor matrix、ancestry path。
- squash/rebase 后原 OID 不包含。
- runtime checkpoint 折叠。
- refs revision 变化导致分页重置。

### 17.3 GitHub 测试

使用可控假 `gh` executable 返回 JSON，不依赖真实网络：

- 未安装。
- 已安装未登录。
- 多账号/多 host。
- repository unresolved。
- permission denied/rate limit/not found。
- PR list/detail/already exists/create。
- Issue list/search/detail。
- login/PR create 取消完整进程树。
- 确认任何返回和日志都不包含 token。

已固化 fake `gh` 的 PR preflight/create/diff 接口测试，覆盖真实临时 Git repository、remote mapping、ahead 计算、发布探测、`--body-file -`、stdin 正文隔离、draft 参数、result URL，以及 PR modified/added/deleted 文件、base/head OID、raw content 和有界输出截断。二进制、非 UTF-8、超限文件的统一 limitation code 由 comparison 接口测试固化。其余错误/取消矩阵随 operation event 阶段继续补齐。

### 17.4 前端测试

- 源码管理不得创建第二棵 workspace 文件树。
- 普通文件跳转复用 `file` resource。
- 浏览目录复用 `file-browser` resource。
- turn/Git worktree/commit/GitHub PR 四种 diff source 使用同一个 viewer。
- PR/Issue detail 使用只读 `WorkspaceFileEditor`。
- PR create body 使用可编辑 `WorkspaceFileEditor`。
- `gh` 缺失时所有 GitHub 操作不可调用，但本地 Git 正常。
- runtime lock 时只读功能可用、写按钮禁用。
- commit 无 staged change 时禁用。
- 单击、Shift、Ctrl/Cmd 和 Ctrl/Cmd+Shift 选择语义。
- 多 Commit 聚合按祖先拓扑连接文件演化链，跨分支等价文件 Patch 去重，内容不同的同路径旁支修改保留多条链并显示短 SHA；同时移除创建后删除的净空变化并跟踪重命名链，Root/Merge first-parent 语义与 file totals 正确。
- 右键短/完整 SHA 与提交归属；同一审阅会话切换文件不增加 Tab。
- 下一差异越过当前文件末尾后进入下一文件，上一差异反向进入前一文件末尾。
- 源码管理 -> Diff -> 返回不得重新请求 snapshot/history，并恢复内部 Tab、分页、多选、详情和 commit 草稿。
- workspace/Git metadata watcher、typed mutation 和长操作完成必须使对应本地领域失效并刷新；不同 worktree 缓存隔离，旧请求不得覆盖新状态。GitHub 查询仍保留自身的显式刷新。
- Stage/Unstage 不请求 snapshot/history，只合并 status-scoped 结果；refs 变化 mutation 的 snapshot/history 必须并行发起。
- 单文件 Stage/Unstage pending 时目标按钮显示 spinner，其他 Git 写入口全部禁用，重复点击不得发起第二个接口请求。
- GitHub capability 使用 `gh repo view <owner/repo> --json ...` positional 参数契约。
- GitHub capability、PR/Issue query/detail 在普通 Tab 往返时命中有界缓存；相同 in-flight 查询只调用一次 API，显式刷新才重新请求。
- PR/Issue 详情 locator 与“概览/文件”子页在打开 Diff、切换右侧 Tab和 renderer 卸载/重挂载后保持不变；返回源码管理不得退回列表。
- 点击 PR 列表项后必须立即显示详情 loading 或缓存详情，不能在列表中无反馈等待网络完成；点击 PR 文件继续立即进入现有 Diff Tab，并由该 Tab 自己加载内容。
- PR Diff source 必须携带 base/head OID；后端不得执行 `gh pr diff --name-only` 或重复 PR metadata 查询，base/head raw content 必须并行读取。
- 同一 PR revision 的同文件重复打开命中 comparison cache；head OID 改变后生成新 key 并重新加载。
- Push remote 切换后关闭并重新打开 Dialog、重新挂载资源仍恢复同仓库上次有效选择；偏好 remote 删除后回退到 upstream 或第一项。
- Git operation 失败后同时展示本地化原因/恢复建议和脱敏的 Git 原始失败原因。
- 四主题、宽/窄右栏和键盘可达性。

历史 Tab 契约测试固定缓存命中时立即恢复；首次加载历史与选择 Commit 聚合详情时立即显示对应局部中间态。Commit 点击必须同步发布 selected/focused/loading，再异步执行详情请求；Merge Commit 多文件 summary 使用端点分组 numstat，不读取每个文件正文。新增 Store 接口测试覆盖标准桌面选择语义、历史/聚合缓存复用及会话清理，Rust 临时仓库测试覆盖 Root、默认 HEAD 分页排除旁支、重复 OID 去重、显式选择边界、同文件演化链终态聚合、删除/重命名端点、totals、Merge 首次进入路径以及 branch/tag contains refs；Diff 导航纯函数测试覆盖同文件 chunk、跨文件与会话边界。

### 17.5 实际页面验证

涉及 UI 的每个阶段必须启动前端，优先 deep link 到目标会话和源码管理资源，通过 Codex 内置浏览器验证：

- status -> diff -> stage -> commit。
- branch/tag/worktree。
- stash create/apply。
- fetch/pull/push 状态和取消。
- Commit 范围/增减多选、按文件终态 Diff 聚合、右键归属和跨文件 Diff 导航。
- `gh` 未安装、未登录、ready 三种状态。
- PR 创建/详情/diff。
- Issue 搜索/详情。

验证完成后清理测试 repository、worktree、后台进程和浏览器页面。

Browser preview 的源码管理 fixture 提供 `origin` 与 `fork` 两个 remote；短 mutation 和长 operation 均保留 700ms 可见 pending 窗口，Stage 会把目标文件移动到 staged，向 `fork` Push 会从 queued 进入与桌面接口同构的 `git.authentication-failed + exitCode + reason` 终态，用于可见交互回归按钮反馈、全局写锁、错误详情、长文本换行和 remote 记忆，不改变桌面生产后端行为。

2026-08-11 已在 Gold-Band 实仓测量 Git 查询基线：status 平均约 110ms、refs 约 160ms、worktrees 约 127ms、stashes 约 223ms、双 remote 查询约 389ms、history 约 330ms。旧 Stage 在 `git add` 前后串行执行 revision 校验、完整 snapshot 和 history，Windows 多进程启动累计约 1.5–3 秒；现已改为 status-scoped result，删除 Stage/Unstage 后无关 snapshot/history 查询，refs 变化 mutation 的 snapshot/history 改为并行。DOM 回归固定目标 Stage 按钮立即显示 spinner、其他文件行操作按钮不渲染，完成后文件移动到“已暂存”；后台 watcher 刷新不锁 commit 草稿，Commit 和长 operation 仍锁住同 workspace 写入口。实际 `gh 2.93.0` 已确认旧 `gh repo view --repo ...` 返回 `unknown flag: --repo`，修正后的 positional `gh repo view diodeme/Gold-Band --json nameWithOwner,defaultBranchRef` 成功返回 `main`。

2026-08-11 已使用浏览器 mock 实际验证 Push 错误与 remote 偏好：切换到 `fork` 后关闭并重开 Dialog 仍恢复选择，刷新页面并重新进入源码管理后也恢复 `fork`；确认 Push 后同时展示本地化身份验证原因、恢复建议和 Git 原始失败原因，窄右栏内长文本正常换行。验证页面、1422 端口 Vite 进程和临时日志已清理。

2026-08-11 已在 Gold-Band 实仓测量 GitHub 基线：`gh auth status` 约 1.95 秒、`gh repo view` 约 1.58 秒；旧 PR 文件 Diff 串行执行 `pr diff --name-only` 约 3.82 秒、PR metadata 约 1.47 秒、base raw content 约 1.38 秒、head raw content 约 1.58 秒，单文件约 8.25 秒。现已用 repository/workspace 有界缓存消除普通 Tab 往返的重复 capability/list/detail 调用，并以 immutable base/head OID 删除前两次整 PR 查询、并行读取两个文件版本；同一实仓文件的并行 base/head 请求约 2.06 秒，首次读取耗时下降约 75%，同 revision 重开 comparison 直接命中缓存。

2026-08-10 已使用浏览器 mock 实际验证 GitHub PR #42：详情“概览/文件”切换、文件统计、点击文件打开现有 `file-diff` tab、CodeMirror unified diff 内容与 `+4/-1` 统计均正确，控制台无 warning/error；验证页面和 Vite 进程已清理。

2026-08-12 已将历史区完整替换为 Commit 主从审阅：删除 Canvas/HTML Git Graph、Checkbox 和关系分析前后端路径，改为标准桌面选择、当前归属和有界 Diff review session。2026-08-12 进一步对齐 IntelliJ IDEA Log：批量审阅只收集显式选中 Commit 的 first-parent Changes，最多 4 个 worker 并发读取后按旧到新执行文件演化链 zip，返回每条文件演化链的最早 before 与最终 after；同一拓扑链中的重复路径只展示一次，创建后删除的净空文件消失，重命名链保留首尾路径。Review 结果按 workspace + revision + ordered OIDs 进入 48 项有界缓存，不预读文件正文；会话清理或 LRU 淘汰时同步移除所属 Review 缓存。

2026-08-12 修正跨分支同路径聚合边界：不再假设一个路径全局只有一条文件演化链，只在 Commit 祖先拓扑可连接时合并；相同功能在不同基线产生的等价文件 Patch 使用 `git patch-id --stable` 去重，真正不同的旁支修改保留独立链并在文件列表显示终点短 SHA。普通父子链无需额外查询；实现用 before-path 索引连接候选、用变更类型与增删统计 signature 分桶，只对同路径跨分支冲突候选执行 ancestry/patch-id 查询。选择上限仍为 32 个 Commit；Patch capture 达到 4 MiB 时保守地不去重，避免截断内容误判；文件列表重复路径计数为单次 O(n)，不引入正文缓存或无界状态。

2026-08-12 已使用内置浏览器实际验证 440px 窄右栏：历史区正确退化为“提交/更改”单栏；Shift 从首项到末项选择 3 个 Commit，Ctrl 可独立移除中间项；右键已选 Commit 保留 selection，并展示短 SHA、完整 SHA 和提交归属；Diff 审阅在同一 Tab 支持跨差异与跨文件导航。最新布局契约为无选择时不挂载 splitter，首次选择同步显示高亮与右侧局部 loading，清空后恢复单栏。

2026-08-11 已补齐 repository/workspace-scoped `GitStateMonitor` 与 operation event subscription：普通文件事件复用现有 workspace watcher，watcher 身份提升为 `projectId + canonical root` 以隔离 linked worktree；HEAD/index/refs/packed-refs 由 typed Git service 定位并独立监听。前端按会话过滤两类事件并二次 debounce，本地 Git 与 GitHub 登录/PR 创建的 operation running/terminal 均通过 Tauri event 推送，包含 command 返回前早到事件合并，删除 350/400ms 完成状态轮询。接口测试覆盖 Git/GitHub operation 事件终态、metadata/workspace 事件去抖、worktree 隔离、导航保留和 watcher 身份；浏览器 mock 再次验证变更 Diff 往返时 commit 草稿与“更改”分区原样恢复，无加载态且控制台无 warning/error，浏览器标签、1422 Vite 进程和临时日志均已清理。

本次历史终态聚合回归：Rust 覆盖重复路径聚合、创建后删除净空、重命名链、显式选择边界和删除文件 before→不存在的 comparison，共 5 项定向测试通过；`cargo check -p gold-band --lib --no-default-features` 通过。Web 源码管理相关 5 个测试文件 / 33 项测试通过，覆盖选择/迟到响应、Review 缓存复用与会话清理、GitHub capability 预热、Diff 导航、browser fixture 和中英文文件数量文案；`npm run web:build`、`cargo fmt --all` 与 `git diff --check` 通过。

2026-08-12 Diff 统计与换行语义回归：真实临时仓库接口测试覆盖同一 tracked 文件 staged/unstaged 分层 numstat，以及 8000 行 CRLF blob 对 LF worktree 仅新增 2 行的 comparison，2 项均通过；后者稳定返回 `+2/-0` 且 before/after 均不含 CR。完整 UI status 只并行执行 staged/unstaged 两次批量 numstat，命令数不随文件数增长；history、revision 校验和 stash/commit 前置判断继续使用轻量 status。Web 源码管理相关 8 个测试文件 / 55 项通过，TypeScript、生产构建、Rust lib/desktop check 与 `git diff --check` 通过。本轮按用户要求未启动前端、浏览器或客户端，实际页面视觉验收由用户执行。

本次跨分支聚合回归：真实 Gold-Band 历史确认 `870e077b → d12b9cd9` 的 `src-tauri/src/commands.rs` 终态为 `+173/-28`，旁支 `0cf78b22` 与 `870e077b` 的该文件 stable patch-id 相同；修复前错误跨基线比较为 `+868/-13`。Rust 7 项 `commit_review_` 测试全部通过，覆盖等价旁支去重、相同统计但内容不同的旁支保留，以及原有净空/重命名/显式选择/删除语义；Web 4 个相关测试文件共 32 项通过，生产构建与 `git diff --check` 通过。内置浏览器实际验证历史选择、聚合文件工作区、Tab 往返缓存和无 Canvas Graph，控制台无 warning/error。

2026-08-12 历史缓存与分页回归：删除普通源码管理 Tab 往返时两帧延迟挂载的伪 loading，缓存命中后立即恢复原历史页、选择和详情。旧 `@tomplum/react-git-log@3.5.1` 白屏已确认为分页边界淡出线才触发 Canvas gradient，而该库把传入的 hex 颜色按 `rgb(r,g,b)` 解析导致 `NaN`；新历史列表不包含 Canvas/Graph 路径。Browser preview 新增 303 条确定性两页 fixture；实测从 300 条首页进入第 2/2 页显示剩余 3 条，再往返“仓库/历史”仍立即恢复第 2 页，无 loading、无 console error/warning。

2026-08-12 当前分支历史范围与点击性能回归：默认历史从 `git log --all` 修正为当前工作树 `HEAD` 的可达祖先链；Gold-Band 实仓基线为 main 698 条、all 839 条，原截图底部实际是 `--all` 的第 298–300 项旁支混排结果。真实临时仓库接口测试固定默认查询排除未合并旁支，并持续分页到 parent 为空的 Root Commit。Commit 行修正 Context Menu trigger 事件边界，点击同步发布 selected/focused/loading；聚合文件统计按首尾端点批量执行 numstat，删除逐文件正文读取的 N+1。内置浏览器实测首项点击立即选中并进入 1 文件详情，第 1 页 300 条、第 2 页 3 条且末页禁用“较早”，控制台无 error/warning。

2026-08-12 大文件 Diff 精度与初始定位回归：Gold-Band `2ab91a05..6b965885` 的 `src-tauri/src/commands.rs` 权威 Git 终态为 `+221/-40`。CodeMirror Merge 默认 `scanLimit=500` 在约 7700 行文件上提前降级，实测把真实约 98 个变化块误渲染为 `+3896/-3715` 的大片变化；提高到 10000 后恢复为 `+203/-22` 的精确 CodeMirror 字符/行块投影，20 次本地算法基线平均约 131ms，并设置 300ms timeout 防止极端输入长期占用主线程。审阅 landing 状态拆为 `top / first-change / last-change`：文件列表和左右文件切换从顶部打开，只有上下差异跨文件时定位首/末变化，消除首次打开直接跳到第 27 个差异的问题。该修改不增加任何后端 Git 命令、正文请求或缓存体积。

2026-08-13 审阅 summary 与滚动状态回归：确认 Git `numstat` 与 CodeMirror diff 对移动/重复代码可能给出不同增删统计，审阅 item 现携带历史 numstat、workspace numstat 或 GitHub PR files API 的领域 summary，列表与 Diff Tab 统一消费；正文算法只渲染 chunks，权威 summary 缺失时才使用 comparison fallback。历史 Commit 列表和聚合文件列表使用 repository/workspace-scoped 独立轻量 scroll offset，相同 review 从 Diff 返回时在 viewport 重挂载并完成布局后恢复文件位置，分页在状态提交前把 Commit offset 归零；scroll handler 只写运行期数字，不发布 React state。鼠标 Commit 点击后主动释放普通 button focus，键盘 focus-visible 保留。以上修改不增加 Git/网络请求、正文解析、缓存条目或重渲染范围；布局后只执行一次常数级滚动恢复。

2026-08-16 修复 unborn history 与空树端点统计：porcelain v2 的 `branch.oid (initial)` 在 Git 协议解析边界直接归一化为 `None`，snapshot 与 history 共享同一 canonical HEAD 事实，空仓库稳定返回空历史页；commit review 的 `beforeOid=None` 统一表示空树，使用当前仓库 `git hash-object -t tree --stdin` 动态获得匹配 SHA-1/SHA-256 object format 的空树 OID，再执行端点批量 numstat，不能把非 Root 的最终 Commit 错当成只与其父提交比较。真实临时仓库接口测试固定“重复保存的同路径从空树累计 +2/-0”和 unborn 空历史语义；正常历史与有 before 端点不增加命令，只有空树比较增加一次常数级 Git 调用，不读取正文、不引入文件级 N+1、缓存或新状态。

2026-08-17 unborn repository 回归：Git porcelain v2 的 `branch.oid (initial)` 在 typed 解析入口统一规范化为 `None`，删除 snapshot 消费端的 sentinel 特判，使 snapshot、history 与 revision 共享同一 HEAD 语义。真实临时仓库接口测试固定 `git init` 后无首次提交时 snapshot 仍可读取未跟踪文件、repository 标记为 unborn 且 history 返回空页；不新增 Git 命令、前端状态、缓存或依赖。

## 18. 最终验收标准

1. 用户可以在右侧源码管理中完成 status、diff、stage、commit、branch、tag、worktree、stash、fetch、pull 和 push。
2. 所有 Git 写操作都经过现有 Git CLI typed service 和统一锁，不存在前端任意参数通道。
3. 源码管理不复制完整文件树，文件浏览与编辑继续由现有文件工作区负责。
4. turn、Git、commit、PR diff 使用同一通用 CodeMirror comparison viewer。
5. PR/Issue 文档和 PR body 编辑复用 `WorkspaceFileEditor` 的 Atomic 能力。
6. 历史区支持标准桌面多选、任意 Commit first-parent Changes 的按文件终态聚合、可验证的提交归属和同 Tab 跨文件 Diff 审阅。
7. `gh` 缺失时 GitHub 区域明确禁用并提供安装入口，本地 Git 完全不受影响。
8. Gold Band 不读取、保存或输出 GitHub token。
9. runtime worktree/lock 与用户 Git 操作不会并发破坏同一 workspace。
10. 所有核心接口都有临时真实 repository 单元测试，UI 有 DOM 契约和实际页面验证。

## 19. 文档同步要求

实施过程中每次代码修改必须同步维护：

- `docs/gold-band/产品设计文档` 中对应的 Source Control/GitHub 产品设计。
- 本文档的阶段状态、接口和验收结果。
- `docs/gold-band/开发计划/功能点todo列表.md`。
- 必要时更新 `gold-band-mvp-plan.md`。

本功能首版不需要新增内置 prompt。若后续增加 AI 生成 commit message、PR title/body 或 Issue 摘要，必须在 `src/prompts/zh-CN/...` 与 `src/prompts/en/...` 下保持一致目录并同步维护，禁止在实现代码中硬编码长 prompt。
