# AI-DYNAMIC 节点产品设计

## 1. 产品目标

AI-DYNAMIC 让模型在运行期间根据任务现状决定 `end`、`single` 或 `fanout`，同时由 Gold Band runtime 保证代码工作空间连续、并行隔离、可恢复和可收敛。

本设计以《AI-DYNAMIC 工作空间树与 Git 基础设施 V2 技术方案》为唯一 workspace 语义来源。旧版 Agent-facing `WorkspaceMode`、`readonly`、节点自选 `main/worktree`、single 回 main、merge 固定回 main和节点结束即清理 worktree 的方案已删除。

## 2. 用户可见行为

### 2.1 Auto 与固定工作流

- Auto 模式需要 Git repository、有效 HEAD 和 worktree 能力。
- 直接包含 AI-DYNAMIC 的固定工作流同样需要上述能力。
- 不包含 AI-DYNAMIC 的普通固定工作流不受 Git 门禁影响。
- 前端可提前检测，后端必须在创建 run 前权威复检。
- 检查失败时不创建 run、不降级为串行、不跳过 AI-DYNAMIC。

### 2.2 Git 引导

- 未安装 Git：提供下载、重新检测、切换工作流和取消。
- 未初始化 repository：只有用户点击后才执行 `git init`。
- repository 没有 HEAD：引导用户在 Git 工作区完成首次提交。
- Gold Band 不自动暂存、不自动创建首次提交，也不替用户选择文件。

### 2.3 运行中的代码世界

- `single`：后继继续使用来源节点的实际 workspace，能看到其 staged、unstaged 和非 ignored untracked 变更。
- `fanout`：runtime 从来源 workspace 的稳定 commit 为每个 child 创建独立 branch/worktree。
- `merge`：回到创建 group 时的父 workspace。
- `acceptance`：与 merge 使用同一父 workspace；其修复 single 也继续使用该 workspace。
- 嵌套 fanout：从父 runtime worktree 创建更深一层 worktree，最终逐层向父 workspace 收敛。

## 3. Agent 输出协议

Agent 只描述工作，不描述文件系统位置：

```ts
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
```

协议不包含 `workspace` 字段。runtime 根据动态图拓扑唯一分配 workspace。

只读意图通过 task 文本表达。当前产品不宣称文件系统级只读保护；未来如需强制能力，应新增独立 AccessCapability，而不能改变 workspace 位置。

## 4. Runtime workspace 模型

### 4.1 WorkspaceState

每个动态运行维护独立 workspace catalog：

- `id`：稳定 workspace 标识。
- `kind`：`main | worktree`。
- `ownership`：`user | runtime`。
- `repoRoot`、`path`、`branch`。
- `parentWorkspaceId`、`createdByGroupId`。
- `forkCommit`、`checkpointCommit`。
- `status`：`active | frozen | merging | merged | released`。

节点仅保存 `workspaceId`。group 保存 `targetWorkspaceId` 与 `childWorkspaceIds`。

### 4.2 拓扑规则

```text
main
  ├─ wt-a
  │   ├─ wt-a1
  │   └─ wt-a2
  └─ wt-b
```

`wt-a1/wt-a2` 合回 `wt-a`，`wt-a/wt-b` 再合回 `main`。该规则由 group 的 `targetWorkspaceId` 表达，不依赖层级特判。

## 5. Checkpoint

- 用户 main 无论 clean/dirty，fanout 都从 main HEAD 创建；runtime 不 commit、不 stash、不 reset 用户 main。
- runtime worktree clean 时直接使用 HEAD。
- runtime worktree dirty 时，在 fork 或 merge 边界创建内部 checkpoint commit。
- checkpoint 捕获 tracked、staged、unstaged 和非 ignored untracked 文件。
- checkpoint 使用 Gold Band runtime identity，关闭签名并跳过 hooks。
- unresolved merge/rebase/cherry-pick 等中间状态会结构化阻塞，不自动修复。

## 6. 生命周期与恢复

- 同一 workspace 同时最多一个 writer。
- fanout 后父 workspace frozen，child 收敛时由 merge 获得写入权。
- 普通 worker 完成、暂停、应用关闭和可恢复失败都不删除 worktree。
- 只有 group acceptance 通过并 closed、整个动态运行成功，或用户明确丢弃时才释放 runtime child workspace。
- resume 前核对 path、worktree、branch 和可解释 HEAD；不允许静默基于 main HEAD 重建替代 workspace。

## 7. 节点执行环境

runtime 在启动节点前直接设置：

- provider cwd = `WorkspaceState.path`
- shell cwd = `WorkspaceState.path`
- 文件根目录 = `WorkspaceState.path`
- workflow-invocation project root = `WorkspaceState.path`

hidden context 只告知当前 workspace 已由 runtime 分配。Agent 不扫描 worktree、不寻找分支、不切换 cwd、不复制父 workspace 文件。

## 8. Git 基础设施

系统 Git CLI 是唯一执行后端：

```text
typed commands / runtime
  -> GitRepositoryService
  -> GitWorkspaceManager
  -> GitCommandRunner
  -> process::background_command("git")
```

该边界复用用户 Git config、credential helper、SSH、attributes、filters、LFS、hooks 和 submodule 行为，并为后续右侧 Git 工作区提供同一套 status/diff/stage/commit/history/fetch 接口与锁语义。

前端不能传任意 Git 参数，只能调用 typed command。

## 9. 错误契约

启动门禁错误：

- `run.git-not-installed`
- `run.git-repository-required`
- `run.git-head-required`
- `run.git-worktree-required`
- `run.git-repository-unavailable`

动态图语义错误保留 code、path、actual、expected、allowedValues、suggestion 与 params，供 proposal repair 和 UI 诊断使用。

`provider.server-unavailable` 等 `RecoveryMode::Auto` 错误使用共享 `RetryPolicy`，默认在初次调用后最多自动重试 3 次。AI-DYNAMIC 自动重试保持原 attempt、logical prompt 与 session mode，不生成 proposal repair prompt；预算耗尽后才收敛为 `Paused + RuntimeAbnormal`。运行时自动恢复与输出协议 repair 是两套独立状态机。

## 10. 验收标准

- single 跨多个节点保持同一 workspaceId。
- fanout child workspaceId 两两不同，且 parentWorkspaceId 正确。
- 两层嵌套 fanout 逐层合回 targetWorkspaceId。
- runtime worktree dirty 时 fork/merge 前生成 checkpoint；dirty main 不被自动改写。
- 暂停和失败保留 worktree；group closed 后只释放 child workspaces，不释放 target workspace。
- workflow-invocation 使用分配 workspace 作为项目根目录。
- Agent schema 与中英文 prompt 不再出现 workspace mode。
- Git 门禁失败不创建 run；普通 worker 工作流仍可运行。
- AI-DYNAMIC provider/runtime 自动重试次数从共享 `RetryPolicy` 推导，并与 proposal repair 预算分别验收。
