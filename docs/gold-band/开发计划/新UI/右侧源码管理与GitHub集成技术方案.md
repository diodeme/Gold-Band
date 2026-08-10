# 右侧源码管理与 GitHub 集成技术方案

## 0. 文档状态

- 状态：已完成（本地 Git 读取/写入、可取消远端与 stash 操作、GitHub capability/login、PR/Issue 查询、可取消 PR 创建、PR diff、提交图、提交详情、关系分析、会话缓存、Git 状态监控与 operation event subscription 均已完成）
- 日期：2026-08-11
- 范围：Gold Band 桌面端右侧源码管理、常见 Git 操作、提交 DAG、GitHub PR/Issue 集成
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
- 完整提交 DAG、单提交详情、两个提交 diff 和任意多个提交关系分析。

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
| 提交 DAG | `@tomplum/react-git-log` 3.5.1 | 已通过 PoC 并采用 HTML Grid renderer；通过本地 adapter 包装 |
| GitHub | 系统 `gh` CLI JSON 输出 | 采用，不引入 Octokit，不读取 token |
| 消息 Markdown | prompt-kit/Streamdown | 仅用于聊天和流式消息，不用于 PR/Issue 文档主体 |

### 4.1 提交图组件

采用 `@tomplum/react-git-log` 的 HTML Grid renderer：

- Apache-2.0。
- 支持 React 19。
- 接受 `hash + parents[] + branch`，能表示 merge DAG。
- 支持分页、主题、自定义提交行和选择回调。
- 可使用自定义行接入 shadcn Checkbox，实现多选提交。

该项目社区规模仍较小，Canvas 渲染仍有作者列出的已知问题，全分支模式需要完整历史，服务端分页模式只保证单一主分支及合入历史，因此仍不把整个组件视为无条件最优解。2026-08-10 的 PoC 已验证 300/1000/5000 commits、周期性复杂 merge、多 parent、300 行分页边界、任意多选与 DOM 数量；三种规模的单次渲染预算均为 5 秒，页面 DOM 固定在单页 300 行。内置浏览器进一步验证 288px 窄栏无横向溢出、多选状态同步且控制台无 warning/error。基于该结果正式采用 HTML Grid renderer，不采用未完成的 Canvas2D。

业务代码不得直接依赖其内部类型。Gold Band 自有 `commit-graph-model.ts` 定义领域模型，`CommitGraph.tsx` 是唯一允许导入第三方包的 renderer adapter：

```ts
interface CommitGraphEntry {
  hash: string;
  branch: string;
  parents: string[];
  message: string;
  committerDate: string;
  author?: { name: string; email?: string | null } | null;
  refs: GitRefLabelVm[];
  runtimeCheckpoint: boolean;
}
```

若未来替换 renderer，只替换 adapter 和展示层，不改变后端 Git history 协议。

不采用：

- 已归档、主要用于模拟 Git 操作的 `@gitgraph/react`。
- 成熟度不足且缺少大历史分页验证的 `git-graph-svg`。
- 强依赖 antd 和另一套 i18n/布局体系的 `@kne/git-graph`。

## 5. 总体架构

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

### 6.4 Commit 与关系分析

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

interface GitCommitRelationsVm {
  selectedOids: string[];
  targetRef: string;
  commonMergeBases: string[];
  pairwise: GitCommitPairRelationVm[];
  mergeEntries: GitCommitMergeEntryVm[];
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
getGitCommitDetail(input): Promise<GitCommitDetailVm>
analyzeGitCommitRelations(input): Promise<GitCommitRelationsVm>
listGitStashes(input): Promise<GitStashEntryVm[]>
```

### 7.2 短操作接口

```ts
executeGitMutation(input: GitMutationInput): Promise<GitMutationResultVm>
```

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

首版只查看和创建，不删除 worktree。

### 8.9 Fetch/Pull/Push

- fetch/pull/push 使用长操作模型。
- 默认 `GIT_TERMINAL_PROMPT=0`，禁止隐藏终端等待输入。
- 允许用户现有 GUI credential helper、GCM、SSH agent/askpass 正常工作。
- pull 默认 `ff-only`。
- merge/rebase 必须用户显式选择。
- push 不提供 force；首次 push 可显式勾选 set-upstream。
- non-fast-forward 返回结构化错误，不自动 pull/rebase。

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

- repository snapshot、history pages、加载状态和稳定错误码。
- 当前源码管理分区、history page、selected OID Set 和 focused commit。
- commit detail、relations detail、详情加载状态和请求 revision。
- commit subject/body 草稿、当前 Git operation 及事件订阅状态。

失效规则：

- 首次无缓存时加载，普通 Right Workspace Tab 切换、打开 Diff 和返回不失效。
- 用户显式刷新时重新读取 snapshot/history；已有可展示数据不退回全屏 loading。
- typed mutation 成功后使用返回 snapshot 并立即刷新 history；长操作结束后立即刷新 snapshot/history。
- `GitStateMonitor` 事件使对应 repository/workspace 会话失效，不允许由每个组件自行轮询。
- snapshot/history/detail 分别维护请求 revision；旧请求完成后不得覆盖较新刷新或操作结果。
- 缓存最多保留 24 个非活跃会话，使用 LRU 淘汰；有订阅或正在执行 Git 操作的会话不可淘汰。

## 11. 提交历史与多选关系

### 11.1 历史加载

- 使用 topo order。
- 初始加载 300 条。
- 后续按 300 条增量加载。
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

- 顶部显示当前分支、upstream、ahead/behind 和同步操作。
- 分区顺序：冲突、已暂存、未暂存、未跟踪。
- 每行显示状态、路径、rename old path、增删统计和可用操作。
- commit composer 紧贴面板底部，包含 subject、可展开 body、commit 按钮。
- 没有 staged change、workspace locked 或存在未解决冲突时禁止 commit。
- stash 放在工具栏菜单，不作为文件行操作。

### 12.6 历史区

- 提交图和提交列表同行对齐。
- 支持 ref、作者、日期、文本筛选。
- 单击选择并查看详情。
- Checkbox 多选后显示关系分析 action bar。
- 窄面板中详情作为同资源内二级视图，不增加嵌套卡片。

### 12.7 仓库区

仓库区包含分支、tags、worktrees、stash 四个子视图，使用 shadcn Tabs/DropdownMenu/Dialog/Command 等 copy-in 组件。

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
6. `gh repo view --repo <owner/repo> --json ...`

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

PR 文件 Diff 使用 typed `GitComparisonSource::GitHubPr`：

1. `gh pr diff <number> --repo <repo> --name-only --color never` 确认文件属于 PR diff。
2. `gh pr view <number> --repo <repo> --json baseRefOid,headRefOid,files` 获取确定的两端 OID 和文件统计。
3. `gh api --hostname <host> --method GET --header "Accept: application/vnd.github.raw+json" <contents-endpoint>` 分别读取 base/head 文件内容。
4. 转换为现有 `GitFileComparison`，点击 PR 文件后打开现有 `file-diff` resource。

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

### 15.2 Git 错误码

至少包含：

- `git.not-found`
- `git.repository-not-found`
- `git.head-required`
- `git.workspace-locked`
- `git.runtime-workspace-restricted`
- `git.auth-required`
- `git.non-fast-forward`
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
- [x] history page 与真实 parents DAG。
- [x] commit detail 和多选关系分析。
- [x] GitStateMonitor 与前端 snapshot subscription。
- [x] `@tomplum/react-git-log` adapter 技术验证与接入。

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

### 阶段 5：右侧源码管理

- [x] 新增 `source-control` resource 与入口。
- [x] 更改、历史、仓库分区。
- [x] 复用现有 file/file-browser resource。
- [x] 提交图窄栏、键盘、多选、主题和 i18n 基础契约。
- [x] 提交详情与关系 action bar。
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

- Git 未安装/非 repository/unborn HEAD/detached HEAD。
- staged/unstaged/untracked/conflict。
- 空格、Unicode、引号、换行文件名。
- rename/copy/type-change/submodule。
- 初次 commit 与普通 commit hooks。
- branch create/switch/rename/safe-delete。
- 分支被 worktree 占用。
- annotated/lightweight tag。
- worktree create 与路径冲突。
- stash create/apply、include-untracked、apply conflict。
- fetch/pull ff-only/push/upstream/non-fast-forward。
- runtime/UI lock 竞争。

### 17.2 历史测试

- 线性历史。
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
- DAG 多选和关系 action bar。
- 源码管理 -> Diff -> 返回不得重新请求 snapshot/history，并恢复内部 Tab、分页、多选、详情和 commit 草稿。
- 显式刷新、typed mutation 和长操作完成必须失效并刷新；不同 worktree 缓存隔离，旧请求不得覆盖新状态。
- 四主题、宽/窄右栏和键盘可达性。

已新增提交图 adapter DOM/性能测试：覆盖 300/1000/5000 commits、复杂 merge、多 parent、300 行 DOM 上限、分页边界和 Checkbox 外部多选；Vitest 中每种规模的单次渲染必须小于 5 秒且总 DOM 节点小于 25000。

### 17.5 实际页面验证

涉及 UI 的每个阶段必须启动前端，优先 deep link 到目标会话和源码管理资源，通过 Codex 内置浏览器验证：

- status -> diff -> stage -> commit。
- branch/tag/worktree。
- stash create/apply。
- fetch/pull/push 状态和取消。
- DAG 搜索、多选和详情。
- `gh` 未安装、未登录、ready 三种状态。
- PR 创建/详情/diff。
- Issue 搜索/详情。

验证完成后清理测试 repository、worktree、后台进程和浏览器页面。

2026-08-10 已使用浏览器 mock 实际验证 GitHub PR #42：详情“概览/文件”切换、文件统计、点击文件打开现有 `file-diff` tab、CodeMirror unified diff 内容与 `+4/-1` 统计均正确，控制台无 warning/error；验证页面和 Vite 进程已清理。

2026-08-10 已使用浏览器 mock 实际验证提交图：288px 右栏中 HTML Grid 图线与提交行对齐且无横向溢出，refs/author/OID/时间保持紧凑展示，Checkbox 选择后外部 OID 集合与“已选择 1 个提交”同步，控制台无 warning/error；验证页面、浏览器标签和 1422 端口 Vite 进程已清理。

2026-08-11 已使用浏览器 mock 实际验证源码管理缓存、提交详情和关系分析：变更文件打开 `file-diff` 后返回时 commit subject、内部“更改”分区和 snapshot 原样恢复且不出现加载态；提交详情打开 commit file diff 后返回时详情原样恢复；两提交关系、目标引用包含状态和两点文件 Diff 均可用，控制台无 warning/error。验证页面、浏览器标签和 1422 端口 Vite 进程已清理。

2026-08-11 已补齐 repository/workspace-scoped `GitStateMonitor` 与 operation event subscription：普通文件事件复用现有 workspace watcher，watcher 身份提升为 `projectId + canonical root` 以隔离 linked worktree；HEAD/index/refs/packed-refs 由 typed Git service 定位并独立监听。前端按会话过滤两类事件并二次 debounce，本地 Git 与 GitHub 登录/PR 创建的 operation running/terminal 均通过 Tauri event 推送，包含 command 返回前早到事件合并，删除 350/400ms 完成状态轮询。接口测试覆盖 Git/GitHub operation 事件终态、metadata/workspace 事件去抖、worktree 隔离、导航保留和 watcher 身份；浏览器 mock 再次验证变更 Diff 往返时 commit 草稿与“更改”分区原样恢复，无加载态且控制台无 warning/error，浏览器标签、1422 Vite 进程和临时日志均已清理。

最终回归结果：Rust `git::` 接口测试 32/32 通过，`cargo check -p gold-band-desktop -j 1` 通过（仅 3 个既有无关 warning），Web 142 个测试文件 / 948 项测试通过，`npm run web:build`、`cargo fmt --all --check` 与 `git diff --check` 通过。最终浏览器 mock 还验证了 GitHub ready 页、PR 创建对话框和 command 返回前事件合并，成功展示“Pull Request 已创建”，控制台无 warning/error；浏览器标签、1422 Vite 进程、D/C 盘临时 Cargo 目录和日志均已清理。

## 18. 最终验收标准

1. 用户可以在右侧源码管理中完成 status、diff、stage、commit、branch、tag、worktree、stash、fetch、pull 和 push。
2. 所有 Git 写操作都经过现有 Git CLI typed service 和统一锁，不存在前端任意参数通道。
3. 源码管理不复制完整文件树，文件浏览与编辑继续由现有文件工作区负责。
4. turn、Git、commit、PR diff 使用同一通用 CodeMirror comparison viewer。
5. PR/Issue 文档和 PR body 编辑复用 `WorkspaceFileEditor` 的 Atomic 能力。
6. 提交图展示真实 parents DAG，支持任意多选关系分析。
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
