# 右侧源码管理与 GitHub

## 1. 产品定位

源码管理是右侧工作区的一类领域资源，为当前项目或明确选中的 linked worktree 提供常见 Git 与 GitHub 操作。它不创建第二套文件浏览器，也不把终端命令暴露给用户。

## 2. 工作区绑定

- 资源身份由 `projectId + workspacePath` 组成。
- 未指定 `workspacePath` 时绑定项目主工作区。
- dynamic child workspace 必须绑定自身 worktree；后端校验其 Git common directory 与项目一致，禁止通过路径参数访问其他仓库。
- 用户切换源码管理资源时，状态、历史、diff 和写操作必须始终使用同一 workspace 身份。

源码管理会话状态独立于右侧面板组件生命周期，按 `projectId + canonical workspacePath` 进入最多 24 项的运行期 LRU。普通 Tab 切换、打开 Diff 或暂时收起右栏不会重新加载，也不会丢失当前分区、历史分页、提交多选、详情或 commit 草稿；Git 写操作成功和 Git watcher 事件才使对应会话重新读取。Stage/Unstage 只回写最新 workspace status 与 repository revision，不读取未受影响的 refs、worktree、stash、remote 或 history；Commit、分支、标签和 worktree等改变 repository 结构的 mutation 才并行刷新 snapshot/history。旧的异步请求不得覆盖较新的刷新或操作结果，不同 linked worktree 的缓存必须隔离。

GitHub capability、PR/Issue 查询和详情同样独立于 React 组件生命周期，按 `projectId + Git common directory + canonical workspacePath` 进入最多 24 个 repository/workspace 会话的运行期有界 LRU；同一 query 或详情的并发请求必须合并。该会话同时持有轻量导航 locator：PR/Issue 分区、已提交的筛选/搜索条件、选中实体的 kind/number 与详情“概览/文件”子页，不能把完整详情或正文复制进导航状态。普通源码管理 Tab 切换、打开 PR Diff 和返回只读缓存并恢复原详情位置，不重新执行 `gh`；用户显式刷新、登录成功或查询条件改变时才按最小领域重新验证。PR 文件 comparison 以 `host + repository + PR number + base OID + head OID + path` 作为不可变 identity，最多缓存 96 项，PR revision 改变后自然生成新资源，不允许旧 Diff 覆盖新 revision。

用户在 Fetch、Push 或 Push Tag 对话框中主动选择的 remote 是仓库级持久偏好，以规范化 Git common directory 为身份保存，因此同仓库的 linked worktree 共享选择。重新打开对话框时按“仍然有效的用户偏好 → 当前 upstream remote → remote 列表第一项”解析默认值；已删除的 remote 不得继续成为可提交值。偏好使用集中、带版本号且有 64 个仓库上限的 schema，不把 localStorage key 散落在组件中。

每个 repository/workspace 会话共享一个 `GitStateMonitor`：普通文件变化复用现有 workspace watcher，HEAD、index、refs、packed-refs 等元数据由 `git rev-parse --git-path` 定位后额外监听。两类事件经过去抖后只刷新匹配会话；LRU 淘汰会释放 watcher。fetch/pull/push/stash、GitHub 登录和 PR 创建等长操作通过 typed operation event 推送 running/terminal 状态，前端不轮询完成状态；本地 Git 操作终态立即刷新 snapshot/history，早于 command 返回的事件也必须合并而不能丢失。

同一 workspace 任意时刻只允许一个 Git 写操作。pending action 由源码管理会话统一保存为结构化 `kind + path`，不得由各按钮维护旁路 loading：单文件 Stage/Unstage 时被点击行的操作按钮持续显示旋转状态，其他文件行的按钮不渲染，Commit 和其他仓库写操作保持禁用；后台只读刷新使用独立 `refreshing` 状态，不禁用 commit 草稿或文件操作。Commit、Fetch、Pull、Push 在各自主操作按钮显示旋转状态，直至权威结果收敛或结构化失败返回。

会话 composer 的分支入口复用同一 Git runner、repository/workspace 协调锁与结构化错误，但使用独立轻量 `GitBranchPickerSnapshot { workspacePath, currentBranch, headOid, revision, dirtyFileCount, operationInProgress, lock, branches }`，不得为了显示选择器读取完整源码管理 snapshot、numstat、history 或 diff。轻量 snapshot 按 `projectId + workspacePath` 进入 App 会话期最多 24 项的有界 LRU；重新挂载先同步恢复旧 snapshot，再在后台校准，因此普通导航不闪回 loading。该 Store 不持久化，也不作为 Git 事实源。`switch` 与 `create-and-switch` 都是 typed Git mutation；后端必须在同一把 Git 写锁内完成 expected revision 校验、checkout 和最终 `HEAD` 读取，并返回新的 snapshot。Merge/Rebase 进行中、runtime 锁占用、目标分支被其他 worktree 检出或 revision 过期均返回稳定错误码。连续快速切换由该锁串行化，后一个请求必须基于前一个请求返回的新 revision；前端 mutation pending 时禁用重复选择与会话提交，失败后重读轻量 snapshot 收敛，不保留乐观分支事实。

## 3. 信息架构

源码管理入口先读取项目级 Git capability，再决定是否加载 snapshot/history。`not-installed` 显示系统 Git 未安装与官方下载入口；`repository-required` 明确显示当前文件夹不是 Git 仓库，并提供“初始化仓库”；初始化只执行 `git init`，不自动暂存或提交目录。初始化后的 unborn repository 是可操作的正常状态：进入“更改”区展示未跟踪文件，历史返回空页，用户自行选择文件并创建首次 Commit。Git porcelain v2 的 `branch.oid (initial)` 必须在领域解析入口规范化为缺失 HEAD，snapshot、history、revision 与 UI 不得各自识别 Git sentinel。`worktree-required / repository-unavailable` 显示各自的恢复建议；只有 capability 可用后完整 snapshot/history 的真实读取失败才进入结构化错误与重试态，不得再把所有情况折叠成“无法读取当前仓库”。探测和初始化必须放到 blocking task，不阻塞桌面 IPC 事件线程。

快速会话选择新工作树但 Git capability 未就绪时，阻塞对话框的恢复动作固定为“取消 / 重新检测 / 使用主工作区 / 打开源码管理”。“重新检测”和“使用主工作区”使用次按钮，“打开源码管理”是唯一主按钮；对话框不再重复提供 Git 下载页入口。点击主按钮通过当前会话作用域的右侧工作区命令，以 `projectId + main workspace` 打开或激活源码管理 Tab，并关闭对话框；紧凑布局必须在 auto-collapse 收敛后自动展开右侧 Sheet，不能只创建隐藏 Tab。用户完成首次提交或其他仓库配置后再显式重新检测。

源码管理包含四个页签：

1. 更改：conflict、staged、unstaged、untracked 分组，文件级和全部 stage/unstage，staged-only commit，以及保存当前更改为 stash。
2. 历史：当前工作树 HEAD 可达的完整提交列表、标准桌面多选、按文件聚合的最终 Diff 审阅、提交归属和分页；其他分支只有用户显式选择对应 ref 时才进入历史范围，默认不得用 `--all` 混入旁支提交。
3. 仓库：分支、tag、worktree 与已有 stash 的查看/应用，不承载日常同步入口。

仓库标题右侧使用一个动态同步按钮和一个独立 Fetch 按钮。`behind > 0` 时同步按钮同时显示 `↓behind ↑ahead`，主动作固定为 Pull；`behind = 0` 时只显示 `↑ahead`，主动作是 Push，`ahead = 0` 时禁用；未设置 upstream 的当前分支显示上箭头并允许首次 Push。Push 遇到 non-fast-forward 只返回结构化错误，不自动 Pull，用户需自行 Fetch、Pull 后再 Push。点击可用入口后进入对应确认/配置对话框，执行中在原入口显示旋转状态，结束后在仓库标题下保留成功、失败、冲突或取消结果，直到用户关闭或开始下一次操作。更改区不重复显示 Fetch/Pull/Push，只在 `…` 菜单提供全部暂存、全部取消暂存和保存为 stash。Git watcher 自动收敛本地工作区和 Git metadata 状态，因此已加载页面不提供普通“重新读取本地状态”按钮；Fetch 不能删除，因为 watcher 不感知远端服务器的新提交，必须由用户显式联网更新 remote refs。

Fetch 的可选 prune 行为对客描述为“移除远端已删除的分支记录”，并明确提示不会删除本地分支或工作区文件；默认关闭，只有用户显式开启才向 operation 传递 `prune=true`。

仓库页内部使用同层级二级 Tabs：`分支 / 标签 / Worktree / Stash`，一次只挂载一个领域列表；选中的 repository Tab 保存在 repository/workspace 会话中，离开仓库页、打开文件或 Diff 后返回仍恢复原分区。创建入口按当前领域提供对应操作，不把四个长列表同时平铺。所有行使用面板宽度作为硬边界：主文案与 SHA/ref/path 在剩余空间内省略，固定操作区不参与压缩，长 Stash message 或 Worktree path 不得产生横向滚动或撑宽客户端。

Worktree 行提供 Git 原生安全删除。当前正在使用的 Worktree 禁止删除；其他 Worktree 删除前必须展示完整路径并二次确认，执行 `git worktree remove` 且不传 `--force`。含未提交或未跟踪改动时由 Git 拒绝并返回结构化原因；删除 Worktree 不删除关联 branch。请求路径必须先与后端 `git worktree list` 的规范化权威路径匹配，不能直接把前端路径作为任意文件系统删除目标。删除中的目标行显示旋转状态并禁用冲突写入口，完成后 watcher/命令结果刷新权威仓库快照。

Pull 采用 Git 原生冲突工作流，不实现三方合并编辑器。`status.operationInProgress` 是 Merge/Rebase 进行中状态的唯一事实源；冲突文件点击后打开普通文件编辑 Tab，用户直接修改冲突块。Merge 显示“完成 Merge / 放弃 Merge”，Rebase 显示“继续 Rebase”，并在危险菜单提供“跳过当前 Commit / 放弃 Rebase”。完成或继续必须先弹确认框；确认后后端在同一 workspace 写锁中读取当前 unmerged 路径，只对这些路径执行 `git add --`，随后调用 `git merge --continue` 或 `git rebase --continue`，不暂存其他普通改动。跳过会丢弃当前正在重放的整个 Commit，确认框必须展示短 SHA 和标题；中止分别调用 `git merge --abort`、`git rebase --abort`。进行中禁止普通 Commit、Pull/Push、Stage/Unstage 和仓库写操作，只允许流程控制动作。

Git metadata watcher 必须覆盖 `MERGE_HEAD`、`REBASE_HEAD`、`rebase-merge`、`rebase-apply` 及 index/HEAD；用户在其他 IDE 中编辑、暂存、继续、跳过或中止后，Gold Band 通过事件去抖重新读取权威 snapshot，不轮询、不保留旁路状态。Rebase 生命周期只由 `rebase-merge` / `rebase-apply` 目录判定，`REBASE_HEAD` 仅用于读取当前 Commit；Git 在成功结束后可能保留该文件，不能据此误报 Rebase 仍在进行。
4. GitHub：GitHub CLI 能力状态、PR 和 Issue。

提交历史与分页定位属于 repository/workspace 会话缓存：首次无数据时使用真实请求中间态；普通源码管理 Tab 往返必须直接恢复已缓存列表，不插入延时或伪 loading，不重新请求 history。已经显示的历史数据在后台刷新时继续保留，不退回全屏 loading。

历史工作区默认只有 Commit 单栏；至少选中一个 Commit 后才挂载响应式主从布局。布局按历史内容区的实际 CSS 宽度判断：达到 `520px` 时进入双栏，左侧 Commit 列表最低 `220px`，右侧聚合变更最低 `280px`，余量留给分隔条；低于该阈值退化为“提交/更改”单栏切换。清空选择后无论宽度都立即恢复 Commit 单栏。单击执行单选，`Shift + Click` 按当前可见稳定 OID 顺序范围选择，`Ctrl/Cmd + Click` 增减选择，`Ctrl/Cmd + Shift + Click` 合并范围；不显示 Checkbox，也不增加独立多选模式。选择颜色与右栏 loading 必须在同一次同步状态提交中出现，不能等待 Review IPC；旧请求不得覆盖新选择。

Commit 列表与当前聚合文件列表是两个独立滚动域，按 repository/workspace 运行期会话保存轻量 scroll offset；打开 Diff Tab 再返回时分别恢复，不能互相覆盖。滚动恢复必须在审阅 viewport 重挂载并完成布局后应用，不能只在数据 identity 变化时设置一次。切换历史页属于新列表导航，Commit 列表必须回到顶部；聚合文件滚动位置只在相同 review identity 下恢复。鼠标点击 Commit 不保留普通焦点方框，键盘 Tab 导航仍保留 `focus-visible` 可访问焦点。

多提交“总 Diff”采用 IntelliJ IDEA Log 的 `collectChanges + zipChanges` 心智：只收集显式选中 Commit 的 first-parent Changes，再按历史从旧到新连接同一文件的演化链。文件路径不是全局唯一演化身份：只有祖先拓扑可连接的修改才进入同一条链；跨分支但文件 Patch 等价的重复修改按 stable patch identity 去重；跨分支且内容不同的修改保留为多条独立链，此时右栏允许同一路径出现多次并显示各自终点的 8 位短 SHA。每条链打开后比较该文件最早相关变化之前的版本与最后相关变化之后的版本，按正常文件行号展示最终 Diff，不按 Commit 分段。创建后又删除且没有净端点的文件不显示；重命名沿 `oldPath → path` 串联。非连续选择不会纳入只由未选中 Commit 触碰的其他文件，但首尾版本之间未选中 Commit 对同一文件的影响可能出现在最终内容中。Root Commit 与空树比较；聚合链的 `beforeOid = null` 同样表示空树，空树 OID 必须由当前仓库的 Git object format 动态生成，不得硬编码 SHA-1。Merge Commit 使用 first-parent Changes。点击 Commit 必须在同一事件链立即发布选中态和详情 loading；聚合文件 summary 按相同首尾端点分组执行批量 numstat，不允许为每个文件读取正文或产生 N+1 Git 命令，正文只在进入 Diff Tab 后加载。Commit 右键菜单提供 8 位短 SHA、完整 SHA 和“查看提交归属”。提交归属只展示当前包含它的本地/远端分支与 Tag、是否进入当前分支、第一父主线/首次 Merge 路径和父提交，不使用 reflog 推断、也不声称能还原历史分支来源。

更改区、历史聚合区和 PR 文件区点击文件都创建同一种审阅会话并打开同一个 Diff Tab。Tab 支持上/下一个差异、上/下一个文件，四个图标按钮都使用 shadcn Tooltip 明确语义；从文件列表首次打开以及使用左/右文件按钮切换时从文件顶部开始，只有在当前文件最后一处差异继续向下或第一处差异继续向上时，才进入下一/上一文件并定位其首/末差异。更改区的 staged 与 unstaged/untracked 分别形成独立序列，PR 使用 immutable base/head OID 下的全部文件。审阅会话只在有界运行期 Store 保存文件 locator 序列和列表已有的权威增删统计，Tab locator 仅保存 `reviewSessionId + reviewItemId`；同一会话切换文件替换原 Tab，不新增 Tab。文件内容按需加载，仅缓存当前和相邻项，迟到响应不得覆盖当前文件。Git numstat/GitHub files summary 与 CodeMirror 正文匹配算法可能对移动代码产生不同统计，列表与 Diff Tab 必须统一展示领域 summary，CodeMirror 只负责差异块渲染，不建立第二统计事实源。

三处文件列表统一使用 `Diff type + path + summary` 行：新增/未跟踪显示绿色 `A`，删除显示红色 `D`，修改、重命名、复制、类型变化和未合并统一显示蓝色 `M`；summary 固定为 `+added -deleted`。工作区 tracked 文件的 summary 来自 staged、unstaged 各一次批量 `git diff --numstat -z`，命令数不得随文件数增长；未跟踪文件在进入 index 前不主动扫描正文统计，避免状态刷新读取任意大小的新文件。历史 summary 来自每条聚合演化链首尾版本的批量 numstat，不能相加单 Commit 统计或逐文件读取正文；PR kind、旧路径和 summary 来自一次分页批量读取的 GitHub PR files API，不能由前端猜测。Stage/Unstage 等纯图标文件操作必须同时提供视觉 Tooltip；已暂存行的回转图标只表示“取消暂存”，Discard 必须使用不同语义和入口。

更改列表是紧凑 Git 领域列表。完整目录浏览、普通文件编辑继续使用现有文件工作区；变更和提交比较复用统一 CodeMirror comparison viewer。

工作区没有冲突、已暂存、未暂存或未跟踪文件时，“工作区没有变更”占满更改工具栏与 Commit composer 之间的剩余内容区并水平、垂直居中，使用弱化前景色表达非阻塞空状态；不得复用带主标题强调的错误/能力提示样式，也不得仅在滚动内容顶部居中。

## 4. Git 操作约束

- 系统 Git CLI 是唯一读写后端。
- 前端只发送 tagged union，不允许发送任意命令或参数数组。
- 用户写操作与 runtime checkpoint/worktree 共用 repository/workspace 协调锁。
- commit 只提交 index，不自动 stage 未暂存文件。
- pull 默认 `ff-only`；不提供 force push、discard/reset、amend 或自动冲突改写。
- runtime 内部分支默认隐藏并禁止直接 push。
- 所有错误通过稳定 `code + params` 返回，UI 负责中英文文案。Git 命令失败的 `params` 同时保留退出码与经过 URL credential 脱敏、空行归一化和长度限制的原始失败原因；源码管理会话不得再把它降级为单独的 error code。
- 失败区紧邻当前仓库标题显示本地化的“原因 + 恢复建议”，其下显示 Git 原始失败原因并允许长文本换行；不得只显示“Git 操作失败”。

## 5. GitHub 能力状态

首次进入 GitHub 页签时依次探测：

- `gh --version`
- `gh auth status --json hosts`
- 当前 Git remote 到 GitHub repository 的映射
- 使用 `gh repo view <owner/repo> --json nameWithOwner,defaultBranchRef` 校验仓库与默认分支；repository 是 positional argument，不使用该子命令不支持的 `--repo` flag

首次探测结果进入 repository/workspace-scoped 运行期缓存；普通页签往返直接恢复，不显示“正在检测”也不启动新 `gh` 进程。只有“重新检测”、登录终态或身份变化显式失效 capability。remote mapping 只读取 status 与 remotes，不为 GitHub 探测重复构造 refs、worktree、stash 和 history 等完整 Git snapshot。

未安装时禁用 GitHub 操作并提供 GitHub CLI 官方安装入口，本地 Git 不受影响。未登录时由用户按钮启动 `gh auth login --web --clipboard`；后台进程隐藏窗口，浏览器完成授权，UI 提供取消与重新检测。应用不读取、保存或输出 GitHub token。

PR/Issue 正文查看和 PR body 编辑复用现有 `WorkspaceFileEditor` Markdown/Atomic 能力；PR diff 继续复用统一 comparison viewer。PR 详情通过一次 `gh api repos/{owner}/{repo}/pulls/{number}/files --paginate --slurp` 批量取得权威 status、previous filename 和增删统计，禁止逐文件查询或前端猜测类型。

PR 列表项点击后先进入由选中 locator 驱动的详情 loading 状态，再异步读取 PR 详情；不得让列表在请求期间保持无反馈，也不得通过延迟切页掩盖请求耗时。PR 详情提供“概览/文件”分区，并随详情返回 base/head OID。文件列表只承担 PR 变更导航，点击文件后打开现有右侧 `file-diff` resource，并由新 Tab 自己展示 comparison loading；typed `github-pr` comparison source 携带后端已经解析的不可变 base/head OID，后端校验 host、repository、OID 和路径后，并行按两个 OID 读取文件内容，不再为每个文件重复执行整 PR 的 `gh pr diff --name-only` 与 `gh pr view --json files`。前端不能传递任意 `gh` 参数。新增、删除、二进制、非 UTF-8 和超限文件继续使用统一 `GitFileComparison` 与 limitation code；加载期间 viewer 显示 spinner。

## 6. 复用策略

- UI：shadcn/ui copy-in 组件与 Tailwind utilities。
- 完整文件树：现有 `FileWorkspacePanel`、`WorkspaceFileTree` 和对应 stores。
- 文件编辑与 Markdown：现有 `WorkspaceFileEditor`。
- Diff：现有 CodeMirror `unifiedMergeView` 展示链路。
- 文本 comparison 在统计和返回正文前统一将 CRLF、CR 规范为 LF，保证右上角 summary 与 CodeMirror 使用同一文本语义；仅换行风格不同不得把整文件判为修改。CodeMirror 默认折叠 unchanged content，只展开差异及必要上下文。大源文件必须提高成熟 Merge 组件默认过低的 diff scan limit，并同时设置主线程计算超时边界；不能因扫描提前降级而把大段 unchanged content 渲染为新增/删除。
- 历史布局与菜单：复用 `react-resizable-panels`、shadcn Context Menu/Dialog 和现有响应式 split 模式；不保留 Git Graph renderer。
- GitHub：系统 `gh` 的结构化 JSON 输出。

## 7. 当前实施状态（2026-08-11）

2026-08-16 起源码管理 comparison surface 接入 Theme Contract v2 的稳定 `diff` role；主题可以通过封闭 recipe 调整表面、边界、形状、阴影和状态色，但不能建立第二套 Diff 统计、文件身份或交互状态。主题切换只更新 CodeMirror theme extension/CSS variables，保持当前审阅会话、请求 revision、滚动位置和 `EditorView` identity。

已完成 Git snapshot、porcelain v2 状态解析、refs/worktree/stash 读取、历史分页、文本 comparison、共享写锁、stage/unstage、staged-only commit、branch/tag/worktree typed mutation，以及右侧工作区的更改/历史/仓库基础视图。历史区默认单栏并在选择后进入响应式 Commit 主从工作区，支持 300 条客户端页面、按需加载下一后端页、标准桌面范围/增减选择、按文件演化链聚合的最终 Diff、右键 SHA/归属操作和跨文件 Diff 审阅会话。

`workspacePath` 已贯穿 snapshot/history/mutation/comparison/长操作 IPC，并由后端校验 linked worktree 归属。stash create/apply、fetch/pull/push/push-tag 已接入可取消后台进程组、共享锁和结构化状态；分支、tag、worktree、stash 与远端操作已提供 shadcn 对话框/菜单入口。

Git 失败链路已完整保留 `code + params.reason`：常见的身份验证、权限、仓库不存在、主机解析、网络不可达、远端拒绝和 non-fast-forward 使用稳定错误码，未细分失败仍展示脱敏后的 Git 原始原因。Fetch/Push/Push Tag 的 remote 选择已按 repository common-dir 持久记忆，重新打开对话框和重新挂载源码管理资源后继续使用上次有效选择。

GitHub 已完成 CLI capability、网页登录、repository/default branch/remote mapping、PR/Issue 列表与详情、带 typed preflight 的可取消 PR 创建，以及 PR 文件 Diff。仓库探测使用 GitHub CLI 支持的 positional repository 参数，并以命令参数契约测试防止回退到无效 `--repo` flag；capability、列表、详情和 immutable-revision comparison 已进入有界缓存并合并 in-flight 请求。预检覆盖 head/base、ahead、发布状态和已有 open PR；未发布分支必须由用户显式 push。PR body 使用可编辑 `WorkspaceFileEditor`，只经 stdin 传给 `gh`。PR 文件列表进入与更改/历史相同的连续 CodeMirror 审阅会话，base/head 内容并行读取；权威文件状态映射和输出截断已有接口测试。

GitHub 列表与详情必须以右侧面板宽度为硬边界，根容器、Tabs、滚动区和行建立完整的 `min-width: 0 / overflow: hidden` 约束链。PR/Issue 标题、账号、head/base 分支及文件路径占用可压缩空间并省略；返回、打开远端、状态 Badge 和增删统计保持固定，不允许任何远端长文本撑宽客户端或制造横向滚动。

旧 Git Graph、Checkbox 多选和两两关系分析已完整删除，包括第三方依赖、前后端模型、命令与测试；不再把不可恢复的“历史分支来源”包装成分析结果。新的 `GitCommitReview` 与 `GitCommitReachability` typed service 分离管理聚合文件终态和当前归属。源码管理 snapshot/history、内部 Tab、分页、选择、审阅和 commit 草稿位于 repository/workspace-scoped 有界会话 Store，Review 结果和 Diff 审阅序列使用独立有界缓存，源码管理会话清理或 LRU 淘汰时同步清理所属 Review 缓存；Stage/Unstage 使用 status-scoped mutation result 局部收敛，refs 变更 mutation 并行刷新 snapshot/history，并通过各领域独立 request revision 阻止 stale response。
