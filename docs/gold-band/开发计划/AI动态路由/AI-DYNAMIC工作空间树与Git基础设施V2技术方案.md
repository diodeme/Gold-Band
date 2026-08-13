# AI-DYNAMIC 工作空间树与 Git 基础设施 V2 技术方案

## 0. 文档状态

- 状态：已实现，作为当前运行时基线
- 日期：2026-08-10
- 范围：AI-DYNAMIC workspace 拓扑、嵌套 fanout、checkpoint、merge、Git 前置检查，以及后续右侧 Git 工作区共用的后端基础设施
- 替代范围：本方案已替代旧 AI-DYNAMIC 方案中 WorkspaceMode、readonly、single 不能进入 worktree、merge 固定进入 main、节点结束即清理 worktree 等规则
- 实现基线：`src/git.rs` 提供 CLI-backed typed Git 服务；`src/dynamic.rs` 提供 workspace catalog；编排器拓扑分配、checkpoint/恢复/清理、启动门禁和桌面引导均已按本文迁移。后续右侧 Git 面板复用该服务边界。

## 1. 结论

AI-DYNAMIC 不再让 Agent 选择 main、readonly 或 worktree。workspace 由 runtime 根据动态图拓扑唯一确定：

1. single 后继继承父节点的实际 workspace。
2. fanout 为每个 child 创建独立子 worktree。
3. fanout group 的 merge 与 acceptance 回到创建该 group 时的父 workspace。
4. 嵌套 fanout 从父 runtime worktree 的 checkpoint commit 创建子 worktree。
5. 最外层 group 的父 workspace 是 main，因此最终合回 main。
6. 节点启动时 provider cwd、命令 cwd、文件根目录和 child workflow 根目录直接设置为分配后的 workspace，不让 Agent 自己寻找或切换 worktree。
7. readonly workspace mode 删除。任务是否只读由任务约束表达；未来若需要强制只读，应单独建设文件与工具权限能力，不能再和 workspace 位置混合。
8. runtime-owned worktree 在 fork 或 merge 边界存在未提交修改时，由 runtime 创建内部 checkpoint commit。
9. 用户 main 即使 dirty 也不自动 commit、不自动 stash、不阻塞 fanout；子 worktree固定从 main HEAD 创建，main 的未提交修改只在最终 merge 时作为目标工作区现状处理。
10. Auto 模式和包含 AI-DYNAMIC 节点的固定工作流必须在启动前通过 Git、repository、HEAD 和 worktree 能力检查；不满足时不启动，也不降级。
11. runtime checkpoint、worktree 管理和后续右侧 Git 工作区统一使用 Git CLI-backed GitService，不引入第二套 Git 语义。

## 2. 问题与根因

### 2.1 已复现的问题

现有运行中出现如下链路：

    implement-hello-world
      workspace = 独立 worktree
      产生未合并实现

    test-hello-world
      workspace = readonly
      实际 cwd = main
      看不到上一节点实现

    repair-hello-world-contract
      workspace = main
      基于错误的主工作区状态继续修复

节点 attempt、artifact、attachment 位于用户运行数据目录是正常行为。真正的问题是后继 provider cwd 没有继承来源节点的代码 workspace。

### 2.2 根本设计缺陷

旧 WorkspaceMode 同时表达了两个不同领域：

- 代码快照位于 main 还是某个 worktree。
- 节点是否应只读。

readonly 实际没有提供文件系统或工具级写保护，却会把节点重新定位到 main，因此既不是安全能力，也破坏了分支链路。

旧状态又把 workspacePath 直接挂在节点上，并根据 nodeId 推导 branch。这样无法表达多个 single 节点共享同一个 worktree，也无法正确表达嵌套 group 合回父 worktree。

因此，本问题不能通过给某个后继节点补 workspacePath 修复，必须把 workspace 提升为独立领域对象并统一管理生命周期。

## 3. 设计原则

### 3.1 代码世界连续性

single 表示在同一个代码世界继续工作。后继节点必须看到父节点所有已提交、已暂存、未暂存和可由 Git 表达的未跟踪变更。

### 3.2 并行必隔离

fanout 表示从同一个稳定快照创建多个并行代码世界。每个 child 使用独立 branch 与 worktree，不共享可写目录。

### 3.3 merge 回父 workspace

merge 的目标不是固定 main，而是创建当前 group 时父节点所在的 workspace。

### 3.4 runtime 决定位置

Agent 只决定 end、single、fanout、任务、profile、provider 和依赖关系，不决定文件系统位置。

### 3.5 runtime 管 Git 机械状态

runtime 负责 branch、worktree、checkpoint、workspace 锁、恢复校验和清理。Agent 负责业务实现、语义合并、冲突判断和验收。

### 3.6 用户 main 不被隐式改写

runtime 不得自动提交、stash、reset、checkout 或回退用户 main。main dirty 时允许从 HEAD fanout，但必须明确记录这一语义。

## 4. Workspace 树

### 4.1 基本拓扑

对于以下动态图：

    a
      fanout -> a1, a2

    a1
      fanout -> a11, a12

实际 workspace 树为：

    main
      ├─ wt1  [a1 chain]
      │   ├─ wt11 [a11 chain]
      │   └─ wt12 [a12 chain]
      └─ wt2  [a2 chain]

合并方向为：

    wt11 + wt12
      -> a1 merge，cwd = wt1
      -> a1 acceptance，cwd = wt1

    wt1 + wt2
      -> a merge，cwd = main
      -> a acceptance，cwd = main

### 4.2 任意层级规则

每个 fanout group 都保存 targetWorkspaceId：

    group.targetWorkspaceId = sourceNode.workspaceId

因此：

- main 中创建的 group 合回 main。
- wt1 中创建的 group 合回 wt1。
- wt11 中创建的 group 合回 wt11。

该规则不依赖层级特判，可以支持 maxGroupDepth 允许范围内的任意嵌套 fanout。

## 5. Agent 输出协议

### 5.1 删除 workspace 字段

DynamicNodeSpec 中删除 workspace：

    interface DynamicNodeSpec {
      id: string;
      kind: 'worker' | 'workflow-invocation';
      title: string;
      task: string;
      provider?: string;
      profile?: string;
      sessionMode?: 'new' | 'continue';
      continueFromNodeId?: string;
      dependsOn?: string[];
      workflowId?: string;
    }

以下字段从 Agent schema、prompt、repair reference 和语义校验中删除：

- workspace.mode
- readonly
- worktree
- main
- single-worktree-unsupported
- fanout.workspace.main-unsupported

### 5.2 只读任务

当前 readonly 没有执行级约束，因此删除 workspace mode 后，Agent 通过 task 明确约束下一节点：

    只读取和验证现有实现，不修改任何文件。

如果未来需要强制只读，应新增 AccessCapability：

    readonly
    readwrite

该能力必须由文件工具、命令权限、变更检测或沙箱执行，且不能影响 workspaceId。

## 6. Runtime 状态模型

### 6.1 WorkspaceState

新增独立 workspace catalog：

    interface WorkspaceState {
      version: string;
      id: string;
      dynamicRunId: string;
      kind: 'main' | 'worktree';
      ownership: 'user' | 'runtime';
      repoRoot: string;
      path: string;
      branch?: string | null;
      parentWorkspaceId?: string | null;
      createdByGroupId?: string | null;
      forkCommit: string;
      checkpointCommit?: string | null;
      status:
        | 'active'
        | 'frozen'
        | 'merging'
        | 'merged'
        | 'released';
      createdAt: string;
      updatedAt: string;
    }

语义：

- main 的 ownership 固定为 user。
- Gold Band 创建的 worktree ownership 固定为 runtime。
- branch、path 和生命周期属于 WorkspaceState，不属于某个节点。
- branch 名和 worktree 短路径根据 workspaceId 生成，不能再根据 nodeId 生成。

### 6.2 DynamicNodeState

节点只保存 workspace 引用：

    interface DynamicNodeState {
      // existing fields
      workspaceId: string;
    }

节点 attempt 存储目录与 workspace path 继续分离：

- attempt、artifact、attachment：用户 Gold Band 运行数据目录。
- provider cwd：WorkspaceState.path。

### 6.3 DynamicGroupState

group 增加 workspace 拓扑：

    interface DynamicGroupState {
      // existing fields
      targetWorkspaceId: string;
      childWorkspaceIds: string[];
    }

targetWorkspaceId 在 group 创建时冻结，后续 merge、acceptance、repair 和 resume 都不得重新推导为 main。

### 6.4 持久化目录

建议新增：

    dynamic/
      workspaces/
        workspace-main.json
        workspace-<short-id>.json

graph.json 中保留 workspace catalog 的完整快照或稳定引用，派生文件用于诊断和恢复。

## 7. Workspace 分配算法

### 7.1 Bootstrap

AI-DYNAMIC bootstrap 继承外层运行的当前 workspace。现阶段通常为 main。

### 7.2 next=end

当前 chain 到达 terminal boundary：

- 不释放 workspace。
- 将当前 chain 对应 workspace 注册为所属 group 的 terminal workspace。
- 没有所属 group 时继续检查 AI-DYNAMIC 完成条件。

### 7.3 next=single

    child.workspaceId = source.workspaceId

不创建 branch，不创建 worktree，不重新解析 main，不因 profile 或 sessionMode 改变 workspace。

### 7.4 next=fanout

1. 接受并持久化 proposal。
2. 获取 source.workspaceId。
3. 对 source workspace 加独占写锁。
4. 根据 workspace ownership 解析 forkCommit。
5. 将父 workspace 标记为 frozen。
6. 为每个 child 创建独立 branch 和 worktree。
7. 写入 child WorkspaceState。
8. 设置 group.targetWorkspaceId 为 source.workspaceId。
9. 设置 group.childWorkspaceIds。
10. child node 引用各自 workspaceId。
11. 释放 repository Git 操作锁，但父 workspace 保持 frozen，直到 group 收敛。

### 7.5 多 workspace 依赖

一个普通节点不能直接同时消费多个不同 workspace 的代码状态。

如果 dependsOn 指向多个 workspace：

- 它们已经通过同一个 group merge 收敛：允许，并使用 group target workspace。
- 尚未 merge：proposal 返回结构化错误，要求通过 fanout group merge 收敛。

禁止根据 dependsOn 顺序任意选择某个 workspace。

## 8. Checkpoint

### 8.1 为什么需要 checkpoint

Git worktree 只能从 commit、branch 或其他 Git ref 创建，不能从另一个 worktree 的脏文件系统状态直接派生。

如果 wt1 的 HEAD 为 C1，工作目录还有 D：

    git worktree add wt11 <wt1-branch>

wt11 只能看到 C1，看不到 D。

因此，runtime-owned worktree 在创建子 worktree前必须把当前完整 Git 可表达状态物化为 checkpoint commit。

### 8.2 Fork 前规则

父 workspace 为 runtime worktree：

    clean
      -> forkCommit = HEAD

    dirty
      -> runtime checkpoint commit
      -> forkCommit = new HEAD

父 workspace 为用户 main：

    clean or dirty
      -> forkCommit = HEAD
      -> 不 commit
      -> 不 stash
      -> 不阻塞

main 的未提交修改不进入 child worktree。这是明确产品语义，不得在 prompt 或 UI 中暗示 child 拥有 main 当前完整文件状态。

### 8.3 Merge 前规则

每个 runtime child workspace 到达 terminal boundary 后：

    clean
      -> terminalCommit = HEAD

    dirty
      -> runtime checkpoint commit
      -> terminalCommit = new HEAD

merge agent 只消费稳定 branch/ref 和 terminalCommit，不依赖跨目录复制脏文件。

### 8.4 Checkpoint 内容

checkpoint 捕获：

- tracked staged changes
- tracked unstaged changes
- 非 ignored 的 untracked files
- 已由 Agent force-add 的 ignored files

普通 ignored untracked files不属于可合并交付物。需要跨节点传递但不应进入 Git 的过程文件必须写入 attachments。

### 8.5 Checkpoint 命令约束

runtime checkpoint：

- 只允许在 ownership=runtime 的 workspace 执行。
- 执行前确认没有其他 writer。
- 检测 unresolved conflict、merge、rebase、cherry-pick 等中间状态；存在时返回结构化阻塞，不自动修复。
- 使用 runtime 专属 author/committer identity。
- 禁用 GPG signing。
- 使用 --no-verify，避免内部快照触发用户提交 hooks。
- checkpoint 后再次确认 workspace clean。
- commit message 与 trailer 明确标记内部提交。

示例：

    Gold Band checkpoint: <workspace-id>

    Gold-Band-Internal: checkpoint
    Gold-Band-Workspace: <workspace-id>
    Gold-Band-Group: <group-id>

最终合入父 workspace 时允许 squash，内部 checkpoint 不应污染用户 main 的最终历史。

## 9. Merge 与 Acceptance

### 9.1 Merge 初始化

merge node 不创建新 workspace：

    merge.workspaceId = group.targetWorkspaceId
    merge.cwd = WorkspaceState.path

runtime 注入按 workspace 去重后的 source 信息：

- workspaceId
- path
- branch
- parentWorkspaceId
- forkCommit
- checkpointCommit 或 terminalCommit
- HEAD
- mergeBase
- dirty status

不能再根据 merge source nodeId 推导 branch。

### 9.2 Merge 职责边界

runtime：

- 准备稳定 source refs。
- 锁定 target workspace。
- 提供结构化 Git 元数据。
- 检测 target 是否在运行期间变化。

merge agent：

- 执行实际合并。
- 解决语义冲突。
- 保留目标 workspace 无关改动。
- 完成必要验证。

### 9.3 Dirty main

当 target workspace 是 dirty main：

- runtime 不自动 stash、commit、reset 或 checkout。
- merge agent 必须同时看到 fanout forkCommit、child terminal commits、当前 main HEAD 与 dirty status。
- 普通 git merge 因本地修改拒绝时，不能覆盖用户文件；应做语义整合或返回结构化冲突暂停。
- main 在 fanout 期间继续变化是允许的，但 merge 前必须重新采集状态，不能使用启动时缓存。

### 9.4 Acceptance 与修复

    acceptance.workspaceId = group.targetWorkspaceId

acceptance 输出：

- next=end：group closed。
- next=single：修复节点继承 targetWorkspaceId。
- next=fanout：从 target workspace 再次 checkpoint/fork，形成新一层子 workspace。

## 10. Workspace 生命周期与并发

### 10.1 写锁

同一 workspace 同时最多一个 writer：

- worker 执行
- checkpoint
- merge
- acceptance 修复
- Git 面板 stage/commit/pull

只读状态查看可以并发，但必须基于明确 snapshot。

### 10.2 Frozen

父 workspace 产生 fanout 后进入 frozen：

- child 并行运行期间禁止新的写节点进入父 workspace。
- 所有 child terminal 后，merge 获得父 workspace 独占写锁。
- acceptance 完成后，父 workspace恢复 active 或继续向其父 group 收敛。

### 10.3 清理

不得在普通 worker 完成时删除 worktree。

只有以下情况允许释放：

- group acceptance 通过并 closed。
- 整个 AI-DYNAMIC 成功且相关 workspace 已合并。
- 用户明确取消并选择丢弃。
- 明确的不可恢复清理流程已保存所需诊断。

暂停、merge failure、应用关闭或可恢复异常必须保留 workspace，供 resume 使用。

### 10.4 Resume

恢复前校验：

- workspace path 存在。
- path 是预期 Git worktree。
- branch 与 WorkspaceState 一致。
- HEAD 与 checkpoint 状态可解释。
- 没有被外部操作切换 branch。

不满足时返回结构化 workspace 错误，不静默创建一个基于 main HEAD 的替代 worktree。

## 11. 节点执行环境

节点初始化时由 runtime直接设置：

    provider session cwd = workspace.path
    shell cwd = workspace.path
    file tool root = workspace.path
    child workflow project root = workspace.path

hidden context 只说明事实和边界：

    当前 workspace 已由 Gold Band runtime 分配。
    所有项目文件操作必须基于当前工作目录。

Agent 不负责：

- 扫描 worktree 列表。
- 根据路径寻找父分支。
- 手动切换 cwd 到另一个 workspace。
- 复制父 workspace 文件到当前目录。

workflow-invocation 启动 child run 时必须显式传入 workspaceId 和 project root override，不能重新使用应用级 repoRoot。

## 12. Git 前置能力

### 12.1 需要 Git 的运行

以下运行必须通过 Git preflight：

- Auto 模式。
- 固定工作流的 nodes 中直接存在 AI-DYNAMIC。
- 上述运行的 start、continue 和 retry。

当前普通 WorkflowDsl 只有 worker 与 ai-dynamic，没有通用子工作流节点。因此固定工作流启动判断不需要扫描所谓普通子工作流引用闭包。

AI-DYNAMIC 内部可以通过 allowedWorkflows 创建 workflow-invocation child run，但外层工作流已经包含 AI-DYNAMIC，Git gate 已经生效，无需额外启动文案。

### 12.2 Preflight

启动前检查：

1. Git executable 可用。
2. 当前目录是 Git repository。
3. repository 存在有效 HEAD commit。
4. Git 支持 worktree。
5. repoRoot、git common dir 与 worktree 状态可读取。

前端可以提前检查以改善体验，后端在创建 run 前必须再次权威校验。

### 12.3 失败行为

- start：不创建 run。
- continue/retry：不推进状态，保留可恢复入口。
- 不把 AI-DYNAMIC 改写成 main 串行。
- 不跳过 dynamic node。
- 不降低 maxFanout 来规避 Git。

### 12.4 错误码

    run.git-required
    run.git-not-installed
    run.git-repository-required
    run.git-head-required
    run.git-worktree-required
    run.git-repository-unavailable

后端返回 code 与 params，前端负责中英文文案。

## 13. 启动对话框

### 13.1 Auto

标题：

    Auto 模式需要 Git

正文：

    安装 Git 并重新检测后即可使用 Auto 模式。你也可以选择其他不包含 AI-DYNAMIC 的工作流。

按钮：

- 打开 Git 下载页面
- 重新检测
- 使用其他工作流
- 取消

### 13.2 包含 AI-DYNAMIC 的固定工作流

标题：

    此工作流需要 Git

正文：

    此工作流包含 AI-DYNAMIC，需要 Git 才能运行。

按钮：

- 打开 Git 下载页面
- 重新检测
- 使用其他工作流
- 取消

### 13.3 未初始化 repository

标题：

    当前文件夹还不是 Git 仓库

按钮：

- 初始化仓库
- 选择其他文件夹
- 取消

初始化仓库必须由用户明确点击。不得自动暂存文件、自动创建首次提交或自动选择需要提交的内容。

### 13.4 没有 HEAD

引导用户打开右侧 Git 工作区完成首次提交，不自动提交整个目录。

所有对话框使用 shadcn/ui Dialog、Button 等 copy-in 组件和 Tailwind utilities，不自研弹窗基础组件。

## 14. Git 基础设施选型

### 14.1 结论

以系统 Git CLI 为唯一 Git 执行后端，Rust 内建立 typed GitService。暂不引入 git2-rs、libgit2 或 gix 作为第二套 Git 执行语义。

理由：

- worktree、branch、merge-base、status、diff 与用户本机 Git 行为一致。
- 复用用户 Git config、credential helper、Git Credential Manager、SSH、attributes、filters、LFS、hooks 和 submodule 能力。
- 避免 embedded Git 与 CLI 在路径、凭据、filter 和 worktree 行为上出现差异。
- 当前项目已经通过 process::background_command 调用 Git，演进成本更低。

未来只有在真实性能数据证明历史或对象读取成为瓶颈时，才评估在同一接口后增加只读加速实现；不能让 status、history 与实际写操作来自相互矛盾的后端。

### 14.2 分层

    UI Git Panel
      -> typed commands / view models
      -> GitRepositoryService
      -> GitWorkspaceManager
      -> GitCommandRunner
      -> process::background_command("git")

GitRepositoryService：

- status
- diff
- stage / unstage
- commit
- history / refs
- fetch / pull / push

GitWorkspaceManager：

- capability probe
- checkpoint
- fork worktree
- inspect worktree
- prepare merge sources
- resume validation
- cleanup

GitCommandRunner：

- 进程创建
- cwd/env/参数
- stdout/stderr/exit code
- timeout/cancel
- 进程树终止
- 诊断采集

## 15. 右侧 Git 工作区兼容设计

### 15.1 Workspace 选择

右侧 Git 工作区必须绑定当前页面或会话选择的 workspaceId，而不是始终绑定 repoRoot。

用户查看 dynamic child 时，应看到该 child worktree 的：

- status
- diff
- branch
- history
- checkpoint

查看 merge/acceptance 时，应看到 group target workspace。

### 15.2 操作权限

runtime 正在持有 workspace writer lock 时：

- 允许查看 status、diff、history。
- 禁止 stage、unstage、commit、pull 和改变 branch。

runtime workspace frozen 时：

- 允许查看。
- 禁止 pull、rebase、reset、checkout 等改变 ancestry 的操作。

fetch 是 repository 级操作，必须获得 repository Git lock。

runtime 内部分支 push 默认禁用或要求显式确认，避免将 gb-dyn 分支意外发布到远端。

### 15.3 Internal checkpoint 展示

右侧历史将带 Gold-Band-Internal: checkpoint trailer 的提交标记为“运行时检查点”，默认压缩显示，与用户主动 commit 区分。

如果用户已经在 runtime worktree 主动 commit，workspace clean 时 runtime 直接使用当前 HEAD，不额外创建 checkpoint。

## 16. Git CLI 契约

### 16.1 命令安全

- 所有外部 Git 调用必须通过 process::background_command()。
- 不经过 cmd、PowerShell 或 shell 拼接命令。
- 参数逐项传入。
- 前端不能传任意 Git args，只能调用 typed API。
- 路径参数使用 -- 分隔或 pathspec stdin。
- 文件列表优先使用 NUL 分隔。

### 16.2 机器格式

优先使用稳定输出：

    git status --porcelain=v2 -z --branch
    git worktree list --porcelain
    git diff --raw -z
    git diff --numstat -z
    git log 使用明确 format 与 NUL 分隔
    git push --porcelain

不得依赖本地化的人类可读 status 文本做业务判断。

### 16.3 长操作

fetch、pull、push：

- 后台执行，不阻塞 Tauri command 或 UI 主线程。
- 提供 operationId、运行状态、输出摘要与取消入口。
- 取消时终止完整进程树。
- credential prompt 不通过黑色终端窗口展示。

pull 必须显式选择 ff-only、merge 或 rebase 策略，不读取不可见默认值后直接执行。

### 16.4 Git 错误

    git.not-found
    git.repository-not-found
    git.workspace-locked
    git.auth-required
    git.non-fast-forward
    git.merge-conflict
    git.hook-failed
    git.operation-cancelled
    git.ref-changed
    git.worktree-create-failed
    git.checkpoint-conflict-state

错误结构至少包含：

    code
    params
    commandKind
    exitCode
    stdoutSummary
    stderrSummary

后端不生成对客文案。

## 17. 锁与一致性

### 17.1 Repository lock

以下操作获得 repository 级写锁：

- worktree add/remove/prune
- branch/ref 创建与删除
- fetch
- checkpoint ref 更新

锁应按 Git common dir 标识 repository，而不是只按传入路径。

### 17.2 Workspace lock

以下操作获得 workspace 级独占锁：

- Agent 可写执行
- checkpoint
- stage/unstage
- commit
- merge
- pull/rebase/reset/checkout

锁状态进入 runtime state 和 Git UI view model，避免 UI 与 Agent 同时写入。

### 17.3 状态更新

Git 操作成功后再原子更新 WorkspaceState。若 Git 已成功但状态落盘失败，恢复流程必须通过 branch/path/HEAD 重新核对，而不是盲目重放创建命令。

## 18. 破坏式迁移

项目处于开发阶段，本方案采用直接替换：

- 删除 DynamicNodeSpec.workspace。
- 删除 WorkspacePolicy 和 WorkspaceMode 的 Agent-facing schema。
- 删除 readonly/main/worktree prompt 指导。
- 删除 single-worktree-unsupported 校验。
- 删除 fanout main/readonly/worktree 组合校验。
- DynamicNodeState.workspace/workspacePath 替换为 workspaceId。
- DynamicGroupState 增加 targetWorkspaceId 与 childWorkspaceIds。
- merge workspace summary 改为按 workspaceId 去重。
- worktree branch/path 改为按 workspaceId 生成。
- 删除节点完成即 teardown worktree 的路径。
- 删除旧 UI workspace mode 展示与配置。

dynamic graph schema 使用独立版本：V2 catalog graph 为 `0.2`，内部 run/node/group/workspace 继续保持领域版本 `0.1`。统一存储读取边界按需识别历史 graph `0.1`，将旧 `workspace/workspacePath` 确定性迁移为 catalog 与 group workspace 拓扑，完整校验通过后原子写回；同一文件的并发迁移串行化，第二次读取必须是无写入的幂等路径。历史 dynamic run、node、attempt/session locator 身份全部保持不变。

迁移不猜测可恢复能力：只有仍由当前 Git repository 注册、路径与 runtime worktree 身份可证明有效且 group 未关闭的旧 worktree 才保留 `active`；缺失、已关闭、共享 main checkout 或其他无法证明安全的旧 workspace 统一记录为 `released`，用于恢复历史会话树但不承诺 resume。读取时仅迁移目标 graph，不在应用启动时全量扫描历史运行；不增加前端兼容层或空态 fallback。

Git HEAD 查询沿用 `GitRepositoryService` 的 `Result` 契约；查询失败时迁移器使用稳定的 `legacy-unknown` 占位提交身份完成历史只读投影，错误分支不增加额外 Git 调用、目录扫描或写入，也不得改变 graph/session 身份与幂等写回语义。

## 19. 实施顺序

### Phase 1：数据与 Git 服务

- WorkspaceState 与 workspace catalog。
- GitCommandRunner。
- GitWorkspaceManager。
- capability probe。
- checkpoint。
- repository/workspace locks。

### Phase 2：协议与物化

- 从 DynamicNodeSpec 和 schema 删除 workspace。
- single 继承。
- fanout fork。
- group target workspace。
- merge/acceptance 继承 target。

### Phase 3：生命周期与恢复

- frozen/active/merging/released 状态。
- terminal checkpoint。
- group closed cleanup。
- pause/continue worktree 保留。
- resume 一致性校验。

### Phase 4：Git 启动门禁

- Auto preflight。
- 固定工作流直接 AI-DYNAMIC 检测。
- start/continue/retry 后端权威校验。
- shadcn Dialog 与 i18n 文案。

### Phase 5：右侧 Git 工作区基础

- workspace-aware status/diff/history。
- stage/unstage/commit。
- fetch/pull/push operation。
- runtime lock 与 UI 操作可用性。
- internal checkpoint 展示。

## 20. 接口级测试

### 20.1 Workspace 继承

- worktree dev 输出 single test，test 的 provider cwd 与 dev 完全相同。
- single repair 继续使用同一 workspaceId。
- profile、sessionMode 和 dependsOn 不改变 workspace。

### 20.2 Fanout

- main fanout 创建不同 child workspace。
- dirty main 不被 commit/stash，child forkCommit 等于 main HEAD。
- dirty main 文件不出现在 child，契约与 UI 提示一致。
- dirty runtime worktree fanout 前生成 checkpoint。
- wt1 fanout 创建 wt11/wt12，forkCommit 包含 wt1 修改。

### 20.3 嵌套 Merge

- wt11/wt12 merge cwd 为 wt1。
- 嵌套 acceptance cwd 为 wt1。
- 外层 wt1/wt2 merge cwd 为 main。
- merge source 按 workspaceId 去重，branch 不按 nodeId 推导。

### 20.4 生命周期

- 中间 worker 完成不删除 worktree。
- paused、merge failure、app restart 保留 worktree。
- group closed 后只释放该 group child workspaces。
- resume 检测 path、branch、HEAD 被外部修改。

### 20.5 Git 门禁

- 无 Git 启动 Auto 被拒绝，且不创建 run。
- 无 Git 启动包含 AI-DYNAMIC 的固定工作流被拒绝。
- 无 Git 启动纯 worker 固定工作流允许。
- 非 repository、无 HEAD、worktree 不可用分别返回稳定错误码。
- continue/retry 重新执行 preflight。

### 20.6 Git UI 协调

- runtime writer lock 存在时 stage/commit/pull 被接口拒绝。
- status/diff/history 仍可读取。
- fetch 使用 repository lock。
- checkpoint commit 被标记为 internal。
- runtime branch push 默认拒绝或要求显式确认。

## 21. 验收标准

满足以下条件才视为 V2 完成：

1. Agent proposal 中不存在 workspace mode。
2. 任意长度 single chain 始终共享同一 workspaceId。
3. 每次 fanout 都创建独立 child workspaces。
4. 嵌套 group 始终合回父 workspace。
5. child 节点通过真实 cwd 直接看到父 checkpoint 代码。
6. runtime-owned 脏 worktree 能稳定 checkpoint 并派生。
7. dirty main 不被自动修改，fanout 明确从 HEAD 开始。
8. merge、pause、resume 和 cleanup 遵守 workspace 生命周期。
9. Auto 与包含 AI-DYNAMIC 的固定工作流在缺少 Git 能力时不能启动。
10. runtime 与右侧 Git 工作区共用同一 GitService、锁和结构化错误体系。
11. 接口级测试覆盖上述契约，并纳入后续回归。

## 22. 最终定义

AI-DYNAMIC 的执行单元不是孤立节点，而是处在 Workspace 树中的节点：

    single 延续当前代码世界
    fanout 从当前稳定快照创建多个隔离代码世界
    merge 把子代码世界收回父代码世界
    acceptance 在父代码世界验证合并结果

Git CLI 提供成熟的版本控制能力，Gold Band runtime 提供 workspace 拓扑、生命周期、并发、checkpoint、恢复和产品接口。Agent 始终直接运行在 runtime 分配的 workspace 中，不再承担寻找、选择或切换 worktree 的责任。
