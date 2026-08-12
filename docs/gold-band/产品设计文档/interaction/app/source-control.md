# 右侧源码管理与 GitHub

## 1. 产品定位

源码管理是右侧工作区的一类领域资源，为当前项目或明确选中的 linked worktree 提供常见 Git 与 GitHub 操作。它不创建第二套文件浏览器，也不把终端命令暴露给用户。

## 2. 工作区绑定

- 资源身份由 `projectId + workspacePath` 组成。
- 未指定 `workspacePath` 时绑定项目主工作区。
- dynamic child workspace 必须绑定自身 worktree；后端校验其 Git common directory 与项目一致，禁止通过路径参数访问其他仓库。
- 用户切换源码管理资源时，状态、历史、diff 和写操作必须始终使用同一 workspace 身份。

源码管理会话状态独立于右侧面板组件生命周期，按 `projectId + canonical workspacePath` 进入最多 24 项的运行期 LRU。普通 Tab 切换、打开 Diff 或暂时收起右栏不会重新加载，也不会丢失当前分区、历史分页、提交多选、详情或 commit 草稿；显式刷新、Git 写操作成功和 Git watcher 事件才使对应会话重新读取。旧的异步请求不得覆盖较新的刷新或操作结果，不同 linked worktree 的缓存必须隔离。

每个 repository/workspace 会话共享一个 `GitStateMonitor`：普通文件变化复用现有 workspace watcher，HEAD、index、refs、packed-refs 等元数据由 `git rev-parse --git-path` 定位后额外监听。两类事件经过去抖后只刷新匹配会话；LRU 淘汰会释放 watcher。fetch/pull/push/stash、GitHub 登录和 PR 创建等长操作通过 typed operation event 推送 running/terminal 状态，前端不轮询完成状态；本地 Git 操作终态立即刷新 snapshot/history，早于 command 返回的事件也必须合并而不能丢失。

## 3. 信息架构

源码管理包含四个页签：

1. 更改：conflict、staged、unstaged、untracked 分组，文件级和全部 stage/unstage，staged-only commit。
2. 历史：真实 parent DAG、提交详情、搜索、分页和多提交关系分析。
3. 仓库：分支、tag、worktree、stash，以及 fetch、pull、push。
4. GitHub：GitHub CLI 能力状态、PR 和 Issue。

更改列表是紧凑 Git 领域列表。完整目录浏览、普通文件编辑继续使用现有文件工作区；变更和提交比较复用统一 CodeMirror comparison viewer。

## 4. Git 操作约束

- 系统 Git CLI 是唯一读写后端。
- 前端只发送 tagged union，不允许发送任意命令或参数数组。
- 用户写操作与 runtime checkpoint/worktree 共用 repository/workspace 协调锁。
- commit 只提交 index，不自动 stage 未暂存文件。
- pull 默认 `ff-only`；不提供 force push、discard/reset、amend 或自动冲突改写。
- runtime 内部分支默认隐藏并禁止直接 push。
- 所有错误通过稳定 `code + params` 返回，UI 负责中英文文案。

## 5. GitHub 能力状态

首次进入 GitHub 页签时依次探测：

- `gh --version`
- `gh auth status --json hosts`
- 当前 Git remote 到 GitHub repository 的映射

未安装时禁用 GitHub 操作并提供 GitHub CLI 官方安装入口，本地 Git 不受影响。未登录时由用户按钮启动 `gh auth login --web --clipboard`；后台进程隐藏窗口，浏览器完成授权，UI 提供取消与重新检测。应用不读取、保存或输出 GitHub token。

PR/Issue 正文查看和 PR body 编辑复用现有 `WorkspaceFileEditor` Markdown/Atomic 能力；PR diff 继续复用统一 comparison viewer。

PR 详情提供“概览/文件”分区。文件列表只承担 PR 变更导航，点击文件后打开现有右侧 `file-diff` resource；后端通过 typed `github-pr` comparison source 固定执行 `gh pr diff --name-only`、`gh pr view --json baseRefOid,headRefOid,files` 和按 OID 读取文件内容，不允许前端传递任意 `gh` 参数。新增、删除、二进制、非 UTF-8 和超限文件继续使用统一 `GitFileComparison` 与 limitation code。

## 6. 复用策略

- UI：shadcn/ui copy-in 组件与 Tailwind utilities。
- 完整文件树：现有 `FileWorkspacePanel`、`WorkspaceFileTree` 和对应 stores。
- 文件编辑与 Markdown：现有 `WorkspaceFileEditor`。
- Diff：现有 CodeMirror `unifiedMergeView` 展示链路。
- 提交图：采用 `@tomplum/react-git-log` 3.5.1 的 HTML Grid renderer，并由本地 `CommitGraphEntry`/`CommitGraph` adapter 隔离第三方类型；提交行、shadcn Checkbox 任意多选、refs/runtime 标识和分页状态由 Gold Band 持有。
- GitHub：系统 `gh` 的结构化 JSON 输出。

## 7. 当前实施状态（2026-08-11）

已完成 Git snapshot、porcelain v2 状态解析、refs/worktree/stash 读取、历史分页、文本 comparison、共享写锁、stage/unstage、staged-only commit、branch/tag/worktree typed mutation，以及右侧工作区的更改/历史/仓库基础视图。历史区已接入真实 parents DAG、300 条客户端页面、按需加载下一后端页和任意多选。

`workspacePath` 已贯穿 snapshot/history/mutation/comparison/长操作 IPC，并由后端校验 linked worktree 归属。stash create/apply、fetch/pull/push/push-tag 已接入可取消后台进程组、共享锁和结构化状态；分支、tag、worktree、stash 与远端操作已提供 shadcn 对话框/菜单入口。

GitHub 已完成 CLI capability、网页登录、repository/default branch/remote mapping、PR/Issue 列表与详情、带 typed preflight 的可取消 PR 创建，以及 PR 文件 Diff。预检覆盖 head/base、ahead、发布状态和已有 open PR；未发布分支必须由用户显式 push。PR body 使用可编辑 `WorkspaceFileEditor`，只经 stdin 传给 `gh`。PR 文件列表会打开现有 CodeMirror comparison viewer，modified/added/deleted 与输出截断已有 fake `gh` 接口测试。

提交图 PoC 已覆盖 300/1000/5000 commits、复杂 merge、分页边界、窄栏和多选。单提交详情、多提交两两关系、共同 merge base、目标引用包含状态、first merge 与两点文件 Diff 已接入 typed service 和现有 comparison viewer。源码管理 snapshot/history、内部 Tab、分页、多选、详情和 commit 草稿已迁入 repository/workspace-scoped 有界会话 store，Diff 往返不再重新加载；显式刷新、mutation、长操作完成和匹配 watcher 事件会立即或去抖刷新，并通过请求 revision 阻止 stale response。Git 与 GitHub operation 均已改为共享事件订阅，repository/workspace-scoped `GitStateMonitor` 已复用普通文件 watcher 并补齐 Git 元数据监听，不再由 React 组件轮询完成状态。
