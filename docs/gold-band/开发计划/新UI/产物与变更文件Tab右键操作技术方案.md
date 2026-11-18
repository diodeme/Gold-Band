# 产物与变更文件 Tab 右键操作技术方案

> 状态：已实施；验证结果及实际边界见第 10 节。
> 日期：2026-10-28。
> 工作分支：`codex/conversation-artifact-search-navigation`。
> 前置方案：[会话产物与搜索结果导航定位技术方案](会话产物与搜索结果导航定位技术方案.md)。本方案追加 Tab 文件操作，不改变前置方案中的会话搜索定位范围。

## 1. 目标与已确认语义

产物继续使用现有右侧工作区 Tab 查看。在产物 Tab、工作流交付附件 Tab、变更文件 Diff Tab 和变更文件历史版本 Tab 上右键，按以下顺序提供操作：

| 操作 | 产物 Tab | 工作流交付附件 Tab | 变更文件 Tab |
|---|---|---|---|
| 复制绝对路径 | 复制实际产物文件的绝对路径 | 复制 manifest 定位到的附件绝对路径 | 复制对应逻辑文件的绝对路径，不复制 CAS 文件路径 |
| 复制相对路径 | 相对于该产物来源 attempt 的运行目录 | 相对于该来源 attempt 的 attachments 目录 | 相对于该文件来源的工作目录 |
| 在文件管理器中打开 | 打开所在目录并选中实际产物文件 | 打开所在目录并选中实际附件文件 | 打开所在目录并选中对应的当前文件 |
| 引用到对话 | 添加实际产物文件的结构化文件上下文 | 添加实际附件文件的结构化文件上下文 | 添加对应当前文件的结构化文件上下文 |

**用户已确认：变更文件的“引用到对话”引用对应文件，读取当前文件内容；现有“引用 Diff”继续保留。**

“引用到对话”沿用文件 chip 和文件下发链路，不向输入框插入路径，不把 Diff 或历史版本正文伪装成文件内容。添加 chip 时只解析路径与文件元数据；发送、排队执行时重新校验，最终向 Agent 下发当前文件引用，不冻结添加时的正文。

本次覆盖 `conversation-asset` 中的 `assetKind=artifact`、`turn-attachment` 工作流交付附件，以及 `TurnFileWorkspacePanel` 展示的 `file-diff`、`file-version`。该 viewer 也承载本地 Git、Commit 和 GitHub PR Diff，因此同样提供对应本地文件的操作；无本地工作目录或文件映射时明确不可用，不为菜单拉取远端文件、checkout 或重建工作树。用户消息附件、输入附件和其他非文件 Tab 不在本次扩展范围。

## 2. 根因判断与复用选择

这是现有资源查看设计的实现补齐：Tab 已经携带明确来源，文件树已有相同菜单，Composer 已有结构化文件引用。无需新增查看窗口、目录导航、Run 定位或另一套引用系统。

需要修正的契约边界是：现有 `PromptWorkspaceFileRef` 只有 `projectId + relativePath`，执行时按接收会话的工作目录解析。它适合现有工作空间引用，但无法表达运行目录产物，也无法固定旧会话变更文件的来源。直接塞入产物路径或旧 worktree 相对路径会引用错误文件。

| 已有能力 | 本次复用方式 |
|---|---|
| `WorkspaceDirectoryContextMenu` 与 shadcn/ui Context Menu | 提取共同的四项文件操作片段；目录菜单保留原有编辑操作，Tab 只使用文件操作 |
| 路径复制 helper | 复用 Windows extended-length / UNC 显示规范及相对路径 `/` 分隔规则 |
| `RightWorkspaceDock`、资源 key 与 commands context | 从被右键的 Tab 取得来源；不依赖当前 activeTab，也不新增 Tab 状态容器 |
| `tauri-plugin-opener` 的 `reveal_item_in_dir` | Rust 完成来源与路径校验后揭示文件，不新增依赖或自行启动 Explorer/Finder |
| `workspace-file-reference-bridge` 与 Composer 上下文区 | 扩展现有文件引用类型，沿用添加、去重、数量限制、删除和焦点行为 |
| 现有运行目录路径 helper、工作根解析、TurnFileStore | 按来源解析目标；不从前端猜测本机路径，不读取 Diff 正文取得文件路径 |

组件交互遵循 Radix Context Menu 的焦点、键盘和碰撞处理契约，文件管理器动作沿用 Tauri opener。Rust opener 方法本身不替业务调用者完成来源授权，因此业务校验必须保留在现有命令边界内。

## 3. 路径与数据归属

### 3.1 产物

- 权威来源是 Tab 已保存的完整 attempt locator 和 artifact name，包含已有的 project/task/run/round/node/attempt、动态节点外层定位信息及 branch 信息；已有 canonical task 身份继续按现有规则校验，不新增一套 ID。
- 实际路径统一由 `GoldBandPaths::artifact_file` 或 `dynamic_node_artifact_file` 计算。前端不能通过标题猜测文件名、追加 `.json` 或拼接动态节点目录。
- 相对路径以对应的普通或动态 attempt 运行目录为根。例如实际文件位于该根下的 `artifacts/review.json`，复制结果就是 `artifacts/review.json`。
- branch 字段用于既有来源和归属校验，不要求按分支创建另一份产物目录。菜单操作不切换分支，不重绑已经打开的产物。

### 3.2 会话变更文件

- 权威来源沿用完整 `TurnFileLocator`、`changeSetId`、`changeId`，从目标 change 读取 `logicalPath`。重命名使用该条记录的目标路径；目标不存在时不擅自改用旧名称。
- 文件访问所需工作根由该来源 attempt 的 canonical 工作位置解析，复用主工作区、run worktree、dynamic workspace 及 released/unresolved 语义，不使用当前选中会话的工作根。
- `changeSet.workspaceRoot` 是展示事实，只能辅助计算记录路径的显示与复制，不能单独授权读取。即使它指向存在的目录，也必须通过来源工作位置及现有工作根验证。
- 历史 Diff/版本仍由 CAS 展示；本菜单针对逻辑文件对应的当前磁盘文件。CAS blob、`before`、`after` 不能用作“当前文件”。

### 3.3 工作流交付附件

- 权威来源是 `turn-attachment` Tab 的完整 attempt locator、branch、changeSetId 和 attachmentId；名称与展示路径只作为 UI 投影。
- 菜单与执行解析只读取 finalized change set metadata，按 attachmentId 找到 manifest 记录的相对路径，再限制在来源 attempt 的 `attachments/` 根内 canonicalize。该读取不迁移旧 schema、不重建 journal、不读取 CAS。
- 相对路径以附件目录为基准；缺失、目录、路径穿越、附件根 symlink 或 symlink escape 时保留可复制的记录路径并禁用揭示 / 引用，不回退 attempt 其他文件或项目根。
- 附件预览必须与 metadata 入口共用附件根 symlink / Windows junction 校验，不能仅在菜单解析时拒绝根链接。正文解析在共享校验后继续检查目标 canonical path 位于附件根内，覆盖子目录链接越界。

### 3.4 Git 变更文件

- `file-diff` 同时包含会话变更与 Git 比较，适配时必须按 resource 是否携带 `gitSource` 区分，不能假定所有 Diff 都有 attempt locator。
- 使用 `projectId + gitSource.workspacePath + gitSource.path` 映射本地文件；工作根按现有 linked worktree / Git common dir 规则校验。
- Commit/PR 的 OID 决定 Diff 展示，不决定当前文件引用的正文；引用仍指向该明确本地根下的目标路径。重命名不自动回退 `beforePath`。

### 3.5 缺失与范围边界

- 文件已删除或工作树已收回：可靠的记录绝对路径、相对路径仍可复制；“在文件管理器中打开”和“引用到对话”不可用。不能冒造 canonical path、打开 CAS 或回退主工作区。
- 只有记录绝对路径、无法确定相对基准时，仅提供绝对路径复制；文件在基准外或 Windows 不同盘时，不生成带越界含义的相对路径。
- 本次文件访问限于来源运行目录及现有受信任工作根。工具记录的范围外绝对路径可以复制，但不能仅凭这条记录获得读取授权；既有显式本地文件链接授权流程继续独立工作。
- 不可用项保留在文件 Tab 菜单中并禁用，按结构化原因提供简短说明；没有可接收文件的 Composer 时禁用引用。只读 viewer 仍可执行这些文件操作，“只读”不等于禁止引用。

## 4. 数据结构与接口

### 4.1 扩展现有文件引用元素

沿用现有 `workspaceFiles` 传输、队列和存储字段作为文件上下文通道，仅将元素扩展为受控来源联合，不再增加 `artifactFiles`、`turnFiles` 等并行列表。实现保留 `PromptWorkspaceFileRef` / `ComposerWorkspaceFileRef` 类型名，不触发无关 API 改名；以下代码仅简写其来源契约。

以下是语义契约，locator 在各层映射为现有领域类型，核心 crate 不依赖 Tauri VM：

```ts
type PromptFileRef =
  | { kind: 'workspace'; projectId: string; relativePath: string }
  | { kind: 'artifact'; locator: AcpAttemptWorkspaceLocator; name: string }
  | { kind: 'turn-file'; locator: TurnFileLocator; changeSetId: string; changeId: string }
  | { kind: 'turn-attachment'; locator: TurnFileLocator; changeSetId: string; attachmentId: string }
  | { kind: 'git-file'; projectId: string; workspacePath: string | null; relativePath: string };

type ComposerFileRef = {
  id: string;                 // 沿用草稿 chip ID，不作为文件身份
  source: PromptFileRef;
  name: string;
  byteLength: number | null;
  mimeType: string;
};
```

- `workspace` 保留现有“按接收会话工作目录解析”的语义；新来源显式指向原 Tab 对应的资源。Git 来源保留本地根与路径，不把比较 OID 写成文件版本引用。
- 新来源复用现有 canonical locator/change identity/workspacePath，没有新增 aggregate、持久根 ID、revision 或授权 token。新增来源分支只解决现有 `projectId + relativePath` 无法表达的两个不变量：运行目录归属和固定来源工作位置。
- chip 的展示路径和一次解析结果属于 UI 投影，不作为提交读取依据。提交只携带 `source`；按来源身份先去重，执行时再按解析后的 canonical path 去重。
- 固定来源文件从消息 chip 打开后，预览标题栏再次引用沿用 `FileWorkspaceResource.source`，使用现有来源 identity 生成 chip ID；不得降级为按接收会话解析的 workspace 引用。受控 `turn-attachment` 的 locator 可为 `external` 且没有 workspace 相对路径，此时以已解析路径的文件名补充展示信息，提交仍只使用原 manifest 来源；交付附件卡片打开预览时同样透传来源。普通目录文件仍使用原有 workspace 语义，不扩大没有固定来源的外部文件引用权限。
- 普通工作空间引用的生产者与消费者统一使用联合类型，不保留另一套旧元素解析 fallback。开发阶段不为旧草稿或旧持久格式增加兼容分支；实施时明确受影响的开发数据重建范围。

### 4.2 轻量元数据解析

新增小范围的文件操作解析入口：

```ts
resolveRightWorkspaceFileTarget(source: PromptFileRef): Promise<{
  source: PromptFileRef;
  projectId: string;
  name: string;
  absolutePath: string | null;
  relativePath: string | null;
  relativeBase: 'attempt' | 'workspace' | 'attachment' | null;
  workspacePath: string | null;
  byteLength: number | null;
  mimeType: string;
  availability:
    | { kind: 'available' }
    | { kind: 'unavailable'; code: string; params: Record<string, unknown> };
}>;

revealRightWorkspaceFile(source: PromptFileRef): Promise<void>;
```

`absolutePath` 是供显示和复制的路径，不是前端可提交的访问凭证。只有解析确认文件存在、为普通文件且满足来源边界时才返回 available。没有可靠路径时返回 null；来源伪造、归属错误与非法标识符直接返回既有 `CommandErrorVm { code, params }`。

打开菜单时才解析目标，取得路径、可用性、文件大小与 MIME；不调用 `showArtifact`、`getFileComparison`、正文 preview 或附件 stat 链路。不签发读取/编辑 grant，不计算文件 revision，不建立 watcher。

会话变更和工作流交付附件只读一个目标 change set 的元数据，并按 changeId / attachmentId 找到记录。现有 `load_change_set` 会在旧 schema 下重建并写盘，菜单不能直接借用它；应从 TurnFileStore 提取只读元数据查询，既有 viewer 的迁移职责不变。无法安全识别的记录返回结构化不可用原因，不借菜单触发修复、全历史扫描或数据迁移。

### 4.3 执行时重新校验

- 复制使用本次菜单已取得的展示路径，成功不提示，失败沿用浮层反馈。
- 文件管理器动作只提交 `source`。Rust 在 blocking pool 中重新校验来源、工作根、存在性和文件边界后调用 opener，防止菜单打开后文件或根目录变化。
- 引用动作也先按 `source` 复核可引用性，再调用当前 Composer 的文件引用命令。前端不提交 `absolutePath` 请求读取。
- 后端复用既有 identifier、branch ownership、工作根与 canonicalize 检查；产物目标必须在计算出的 attempt 根内，工作文件必须在受信任工作根内。拒绝路径穿越、符号链接越界、目录冒充文件及 worktree 释放后的根替换。
- 错误使用现有 `CommandErrorVm { code, params }`。能复用既有错误码时复用；新增码集中在文件引用所属模块，前端按 i18n key 展示，不由 Rust 拼接对客文案。

## 5. 引用链路必须完整贯通

不能只实现“点击后多一个 chip”。现有文件引用链路中的以下边界都必须保留来源联合：

1. 首页与会话 Composer 草稿、bridge、上下文 chip、删除、去重和可发送判断。
2. `createUserPromptSubmission`、新会话创建、追问、队列与重试，防止重新映射为 `projectId + relativePath` 丢失来源。
3. `TaskPromptInput`、`authoring/task-prompt-input.json`、相关定时任务 authoring 校验及 fingerprint；原有 null 沿用、空集合清空语义不变。
4. Worker、dynamic session、队列 dispatch 等现有下发入口；在核心应用现有解析流程中按来源分派，Tauri 不独占来源解析能力。
5. 用户消息 timeline 的文件摘要、引用数量及点击打开。artifact chip 回到固定来源产物 Tab；会话/Git 文件先由固定来源 resolver 确认 available，再以来源自己的 `workspacePath` 解析 canonical locator 并打开当前文件，复用已有工作空间文件 Tab。接收会话目录根缺失或不可用不能阻止该文件打开；文件详情和来源 watcher 跟随来源根，目录树与 `browseMain` 继续跟随接收会话。文件不可用时按错误反馈，不改用历史正文。

所有入口继续共享现有“文件引用＋普通附件最多 10 项”限制，不新增引用预算、持久文件副本或第二套队列。最终沿用 `AcpContentBlock::File` 下发路径与文件大小；引用解析本身不读正文、不复制到 `user-inputs`。文件在添加后被修改时使用执行时的当前文件；被删除或越界时按既有 turn 生命周期结算结构化错误。

## 6. 菜单交互与生命周期

- 菜单绑定被右键的 Tab。对后台 Tab 复制路径或打开文件管理器不激活 Tab、不挂载正文 panel、不改变当前分支与文件选择。
- Tab 常规点击、关闭、中键等原行为保持；菜单使用现有 shadcn/ui copy-in 组件，支持组件已有的键盘打开、方向键、Escape 与焦点恢复。窄布局已有可见 Tab 同样可右键，不另外增加面板或工具栏。
- 菜单实例归稳定的 Tab 按钮。流式消息和无关内容更新不能卸载菜单；只订阅已有低频 commands/目标资源，不让所有 Tab 订阅正文或整个 Composer 草稿。
- 元数据请求只绑定本次 resource 来源及局部请求生命周期。关闭菜单、关闭 Tab、资源被替换后丢弃迟到结果；不新增跨层请求身份模型。
- Git review 可能在同一 Tab key 内切换文件，因此执行前必须复核完整来源仍等于打开菜单时的来源；仅检查 Tab key 不足以确认目标。
- 引用命令绑定发起时的 Composer 注册对象。异步校验期间若该对象注销或切换到其他 Composer，则丢弃结果，不向新对话自动添加 chip。复用 bridge 注册生命周期，不另存“当前会话”状态。
- 添加、重复、超限、不可用沿用 bridge 的结构化返回；dock 添加成功后聚焦 Composer，Sheet 内不抢焦点。文件引用与“引用 Diff”互相独立，删除一个不影响另一个。
- 四项名称复用 `workspace.filesPanel` 现有 key。新增的禁用原因、相对路径基准 Tooltip 与错误码翻译按 `ui-locale.md` 同步所有项目语言，不在页面加入实现原理说明。

## 7. 实施顺序与改动范围

1. 先为来源解析建立接口失败测试：运行目录产物、动态产物、会话变更、已收回 worktree，以及同名文件存在于不同根的场景。
2. 定义来源联合和轻量目标 DTO，补齐 Rust 只读 metadata resolver；复用既有可信根校验和 opener，保证不读取正文、不触发迁移写入。
3. 将来源联合贯通现有文件引用生产、持久化、排队、执行和 timeline；保留普通工作空间引用的原行为及 Diff quote 行为。
4. 提取四项共同菜单操作，接入 `RightWorkspaceTabButton`；由 resource 适配器生成来源，不将解析逻辑塞进各 panel。
5. 同步产品设计、开发计划及所有相关语言，完成接口回归、前端构建、浏览器和桌面验证。

主要前端位置：`RightWorkspaceDock.tsx`、`right-workspace-context.tsx`、`WorkspaceDirectoryContextMenu.tsx`、`workspace-file-reference-bridge.tsx`、`composer-context.ts`、两个 Composer、timeline 文件 chip 及 API/types。

主要 Rust 位置：Tauri 文件命令、现有文件引用解析与 Prompt 输入链路、`TurnFileStore` 只读元数据查询；`GoldBandPaths` 和现有工作根模型直接复用。无需修改内置提示词或新增依赖。

## 8. 验收与回归计划

| 层级 | 必须固化的验收 |
|---|---|
| Rust 接口单元测试 | 普通/动态产物真实路径；完整来源与归属校验；变更逻辑路径；主根/worktree/dynamic 根区分；非法 ID、路径穿越、目录、symlink escape、释放根和未授权外部路径 |
| 只读与加载契约 | 菜单只读目标 metadata 和一次目标 stat；不读产物/CAS 正文、不生成 revision、不迁移写盘、不枚举其他 attempts；前端后台 Tab 不挂载 viewer |
| 引用全链路 | 各来源经 submission、队列、持久化、Worker/dynamic dispatch 后仍解析到原文件；切换接收会话/分支不重绑来源；当前文件修改与删除；canonical 去重及共享 10 项上限；普通工作空间引用和定时任务更新语义保持 |
| 前端组件测试 | 两类 Tab 的四项顺序；被右键对象正确；复制不激活；缺失/无 Composer 禁用；关闭与替换后的迟到结果；同 key 的 Git review 切文件；Composer 切换期间的引用隔离 |
| 引用语义 | 当前文件引用和现有 Diff quote 可同时添加、分别删除，发送后分别得到文件上下文和 Diff 引用；不把历史 before/after 当当前文件 |
| 浏览器交互 | 启动前端并优先 deep link；依次尝试 iab、已连接 Chrome；验证键盘、窄宽度、滚动 Tab、流式更新时菜单稳定与焦点行为；夹具测试明确标注边界 |
| 真实桌面 | Windows 上验证剪贴板绝对/相对路径、UNC/extended-length 规范和文件管理器选中文件；真实产物及 worktree 文件引用成功。浏览器夹具不能代替此项 |

实施时先确认失败，再用同一接口测试验证通过；完成相关前端测试、TypeScript 与生产构建、Rust 定向测试。验证结束清理本次页面、测试进程和临时资源；实际结果单独记录，不把前置搜索需求的测试结果作为新菜单的验证。

## 9. 方案自评审

### 9.1 过度设计评审

复用现有 Tab、资源身份、shadcn/Radix 菜单、opener、Composer bridge、文件输入与队列。新增内容仅为共享操作片段、按需 metadata/reveal 入口及文件引用来源分支；没有新 aggregate、持久文件身份、状态机、缓存、监听器或文件副本。

来源分支是必要契约扩展：原模型无法表示运行目录及固定来源工作位置，若省略会把文件引用到错误根。`workspaceFiles` 通道继续复用，不为命名迁移 API；Git 只处理本地文件映射，不扩展远程下载、版本恢复和工作树管理。复杂度与实际需求匹配。

### 9.2 性能影响评审

- 加载按用户打开菜单触发，不为 Tab 列表预读路径，不轮询、不扫描整个仓库或运行历史。产物与会话变更读取来源 `run.json` 校验身份；产物计算目标路径并检查一个文件，会话变更额外读取一个 manifest 并查找一个 change，复杂度为 O(M)，M 为该 change set 的条目数。动态节点额外读取所属 graph 的 canonical 工作空间元数据，查找该节点及工作根，不枚举其他运行或读取消息历史。
- 当前 `turnFiles.captureMaxEntries=256`、单文件捕获上限 2 MiB、总捕获上限 16 MiB；这些是既有捕获边界，不把字节上限误当作 metadata 每次必须读取的正文量。新入口不读取 CAS blob，旧记录不能安全识别时直接不可用，不通过重建扩大读取范围。
- 核心来源解析复用 `GitSourceControlService::resolve_scoped_workspace` 的 linked worktree / common dir 校验，不使用 Tauri 文件编辑 runtime 的 32 项缓存，也不新增持久根缓存。非主工作根可能执行既有 Git 命令；菜单及揭示文件的解析位于 blocking pool，动作显示局部忙碌状态，不阻塞事件线程、不持有全局锁。主根仅 canonicalize；工作树校验只处理实际来源，不扫描文件树。
- 文件引用继续受共享 10 项上限约束。admission/dispatch 只处理实际提交的引用，按规范来源及 canonical path 去重；不增加无界缓存、队列或并发任务。
- UI 变化局限于被操作的 Tab 菜单和目标 Composer 上下文；不扩大会话历史订阅，不因菜单打开重新解析 Markdown 或挂载后台 CodeMirror。
- 队列恢复先展示草稿，附件和最多 10 个固定来源文件的元数据并行补齐后，通过既有 `replaceIfUnchanged` 一次更新；不阻塞文本恢复，不保留新的后台队列。失败保留来源，迟到结果不能覆盖已编辑草稿。

验收以加载次数、文件读取集合和组件挂载边界的接口测试为主，无明显算法或数据增长风险，不为了形式新增 benchmark。若实际验证出现菜单延迟或流式重渲染扩大，再依据生产构建时间线定位原因，不预先加入缓存或并发机制。

## 10. 实施与验收记录（2026-10-28）

### 10.1 已完成范围

- `RightWorkspaceFileMenu` 将共用菜单绑定到产物、工作流交付附件、会话变更 Diff / 历史版本，以及有明确本地文件来源的 Git 比较 Tab。`WorkspaceFileActionItems` 同时复用于文件树，路径复制与既有 Windows 路径规范保持一致。
- Rust 来源解析集中在 `src/provider/workspace_files/sources.rs`；Tauri 的 `resolve_right_workspace_file_target`、`reveal_right_workspace_file` 复用它。菜单只查询目标元数据，揭示文件前重新校验，随后调用官方 opener。
- 文件来源联合贯通 Composer、提交、prompt input、队列持久化与恢复、fingerprint、执行解析、Agent file block 和 timeline。仍使用原有 `workspaceFiles` 通道及与附件共享的 10 项上限。变更文件引用当前物理文件，已有“引用 Diff”保持独立。
- 打开链路把固定来源文件的 provenance 写入文件资源：来源根负责读取、locator 与 watcher；文件 Tab 的目录根仍是接收会话投影。接收 worktree 释放时，来源文件与目录不可用状态同屏展示，显式“浏览主工作区”只切换目录根，不替换或丢弃来源文件。
- 菜单关闭、Tab 关闭、同 key Git review 切换文件和 Composer 注销后，迟到响应不能应用到新目标。队列恢复先还原草稿，再通过原有 `replaceIfUnchanged` 一次补齐附件和文件展示元数据，保留用户编辑。
- 产品设计同步维护于 `docs/gold-band/产品设计文档/interaction/app/workspace-files.md` §2.3。未新增依赖或修改内置提示词；未提交、推送 Git 变更。

### 10.2 自动化验证

| 验证 | 实际结果与覆盖 |
|---|---|
| Rust 定向接口回归 | `workspace_file` 22 项、`fixed_file` 2 项、`prompt_bundle` 5 项，共 29 项通过。覆盖普通/动态产物固定来源、run/task/branch 校验、真实 linked worktree、当前文件修改/删除、释放根不回退、外部根拒绝、旧 manifest 只读不迁移、不依赖 CAS、来源去重、共享上限、队列与 fingerprint 保留来源及 file block 投影 |
| 前端定向回归 | 14 个文件首轮 109 项通过；菜单随后补充 5 项独立用例并定向通过，累计 114 项。菜单文件最终共有 12 项已验证用例，包含菜单顺序、缺失与无 Composer 状态、复制与受控 reveal、失败浮层、迟到响应、同 key 切文件和快速响应后的输入框焦点；其他回归覆盖来源映射、提交、队列恢复、bridge 隔离及 timeline |
| 2026-10-28 打开链路回归 | 先复现 Git / turn 固定来源在接收 worktree 为空时 `selectedFile=null`，以及 viewer 直接整屏显示目录不可用；修复后同一用例验证来源根解析、locator、根投影切换、来源 watcher 与不可用目录同屏。新增 UI 回归覆盖长路径、来源读取、显式浏览主工作区不丢文件 |
| 核心与桌面检查 | `cargo check -p gold-band --lib`、`cargo check -p gold-band-desktop` 通过 |
| 前端生产构建 | `npm run web:build` 通过 TypeScript 与 Vite 构建；仍有既有大 chunk 提示 |
| Windows 桌面构建 | `cargo build -p gold-band-desktop` 通过；仍有 21 项既有编译警告 |

来源保留与解析契约先通过失败测试确认，再用相同测试验证通过。后补的快速响应焦点用例首次即通过，没有据此引入额外焦点修复或状态。

### 10.3 界面与真实客户端验证

- **内置浏览器**：启动前端，使用真实 `RightWorkspaceProvider`、Dock、菜单与 bridge，metadata API 为受控夹具。验证四项顺序、后台 Tab 不激活、路径剪贴板、产物与变更文件 chip、Composer 焦点、Shift+F10 / End / Enter / Escape、每 100ms 刷新时菜单稳定、420×760 窄窗口与溢出 Tab，以及真实浅色/深色主题 token。缺失文件保留路径复制，禁用揭示文件与引用。
- **2026-10-28 打开链路夹具**：使用真实 `RightWorkspaceProvider` 与 `FileWorkspacePanel`，接收 worktree 设为 released，Git 来源指向可用 source worktree。验证来源文件正文显示、来源根 watcher 启动、目录侧显示释放状态；点击“浏览主工作区”后目录切到主根而来源文件仍在。长相对路径在 390px 窄面板切换“文件 / 目录”，拉宽后恢复双栏且正文不丢。页面控制台无错误。
- **真实 Windows EXE**：使用独立临时 `GOLD_BAND_HOME`、实际运行目录产物、Git 仓库及真实 linked worktree；调用真实 desktop IPC resolver 与 opener。确认产物绝对路径来自原 attempt，Explorer 打开并选中 `review.json`，产物和变更均生成文件 chip。将来源 run 指向 linked worktree 后，复制结果为该工作树的 `src/current.ts`，引用成功，未回退主仓库同名文件。viewer 和 Composer 接收展示使用隔离夹具，未宣称完整生产会话端到端发送验证。
- **清理**：已停止本次启动的 EXE 进程树和两台 Vite，删除临时前端入口及隔离运行数据；有效验证页面已关闭，其他临时页面由内置浏览器在回合结束时回收。临时仓库只剩只读/隐藏 Git 元数据，强制清理被自动审批拒绝（仅返回 `blocked by policy`），留在系统临时目录。保留原有用户窗口与无关工作区变更。

### 10.4 验证边界与自评审结论

- 实际桌面验证使用 Windows 本地磁盘。UNC / extended-length 的显示与复制规范由 helper 单测覆盖，未连接真实网络共享；符号链接越界继续依赖现有 canonicalize 边界，本轮没有在 Windows 单独创建符号链接验证。
- 未向真实远端 Agent 发送测试消息；执行时文件来源解析、当前文件语义及 `AcpContentBlock::File` / ResourceLink 投影由接口测试验证。Diff 与历史版本正文仍由原有 viewer 链路提供。
- 开发阶段直接替换旧的无 `kind` 的 `workspaceFiles` 元素格式，不增加兼容读取或迁移。旧开发数据中的文件引用需重新添加；没有为此更改普通消息正文或新增迁移任务。
- **过度设计验收**：复用现有 Tab、Radix 菜单、opener、Composer、文件输入与队列；新增抽象只覆盖来源差异和共享操作，没有新增持久身份、缓存、状态机、后台队列或文件副本，复杂度与固定来源文件操作相匹配。
- **性能验收**：菜单按需读取单个来源的身份和元数据，不读取产物/CAS 正文、不扫描运行历史、不挂载后台 viewer。工作根校验放在 blocking pool，引用继续受 10 项上限约束，UI 更新限定于菜单和目标 Composer。浏览器持续刷新期间菜单保持稳定；未发现需要引入缓存或额外 benchmark 的性能问题。

### 10.5 工作流交付附件卡片入口复核（2026-10-28）

- 用户反馈的入口明确为工作流交付附件卡片。复核时，当前工作区已有 `turn-attachment` 来源适配及 Rust finalized manifest 解析，原菜单测试 13 项首次运行全部通过；未建立产品缺陷的失败证据，因此本轮没有增加业务特判、状态或另一套附件菜单。
- 新增 `web/tests/workflow-attachment-tab-menu.test.tsx`，从真实 `TurnFileChangesCard` 点击附件，经真实 Provider、Dock、Radix 菜单和 Composer bridge，逐项验证四项中文操作、附件根相对路径、受控 reveal 与结构化引用携带的完整 manifest 来源。API 使用夹具；不将其视为真实 EXE 或 Agent 发送验收。
- 内置浏览器使用同一入口与真实组件验证，正常宽度和 390px 窄窗口下菜单均完整显示，长附件名截断正常；文件管理器夹具收到调用，引用结果正确。最初的遗留验证页缺少 `TooltipProvider`，右键后报错空白，该现象属于验证环境错误，未被作为产品根因。
- 检查时 `web/dist/index.html` 的生成时间为 14:50，早于 15:59 的附件来源适配源码；旧构建不能作为当前源码的验证结果。本轮 `npm run web:build` 已通过 TypeScript 与 Vite 生产构建，仍有既有 chunk 大小及动态/静态导入提示；未重新打包桌面 EXE，也未据此认定旧构建就是用户客户端的根因。
- 菜单、卡片到 Tab 集成、附件 viewer 和文件引用四个测试文件串行联跑，共 22 项通过。新集成测试首次单跑通过；与生产构建并发联跑时曾触发默认 5 秒测试超时，同一用例随后串行通过，该超时未被作为产品缺陷证据。Rust 附件来源定向测试因已有编译占用而等待 artifact lock，已取消；本轮未改后端，不宣称新增 Rust 验证通过。
- 已关闭本轮内置浏览器页面，恢复 viewport，停止本轮 1432 端口 Vite 并删除临时验证入口；保留原有验证页及其他会话进程。
- 过度设计与性能复核：本轮只增加入口集成回归及验收文档，无运行时代码、依赖、状态、缓存或数据加载变更，因而不增加生产运行成本。既有规则已要求真实用户入口、DOM 与接口回归，无需另增协作规则。

### 10.6 附件根边界与标题栏再次引用修复（2026-10-28）

- 修复前通过 `TurnFileStore` 公共接口复现：真实 finalized 附件根替换为 Windows junction 后，metadata 返回 `turn-files.attachment-access-denied`，预览却成功返回外部文件路径。预览改为调用已有共享根校验后，同一测试通过；另一项回归确认附件子目录链接仍不能越界，正常附件在替换前仍可读取。`cargo test --test turn_attachment_trust_boundary --jobs 2` 共 2 项通过，`cargo test --lib acp::turn_files::tests --jobs 2` 的现有 32 项也全部通过。
- 修复前 Git / turn-file 的接口映射和真实文件面板 DOM 测试均失败，实际提交来源被改为 workspace。标题栏透传已有 `resource.source` 后，同一测试转绿；完整嵌套 locator、Git 工作位置、普通 workspace 引用及任意外部 locator 拒绝行为都有回归覆盖。文件面板、引用映射、菜单、工作流附件入口、bridge、Composer、草稿与只读 Composer 共 8 个测试文件、54 项通过。
- `npm run web:build` 的 TypeScript 与 Vite 生产构建通过；仍有既有的大 chunk 和动态 / 静态混合导入提示。
- 内置浏览器导航超时，且无已连接 Chrome，按项目工具顺序改用独立 agent-browser 会话。夹具使用真实 `openWorkspaceFileReference`、文件面板、CodeMirror、Composer bridge 与引用 chip，仅替换文件 API；接收工作树设为 released。正常宽度下 Git 再引用保留来源 worktree；390×760 窄窗口下 turn-file 再引用保留完整 locator，长路径截断且按钮可见、无页面横向溢出；切换目录后拉宽到 1200×760，来源正文、引用 chip 和已收回目录状态均保留。本次未重新打包或验证真实 EXE，也未向远端 Agent 发送消息。
- 过度设计与性能复核：复用已有根校验、来源类型、identity 和 UI 组件，没有新增持久字段、状态、缓存或依赖。预览仅增加一次根目录 metadata 检查，引用继续只传轻量来源；没有新增全量扫描、正文读取或订阅范围。现有 `state-lifecycle-and-data-integrity.md` 已覆盖完整作用域和软链接边界，无需新增经验规则。
- 已关闭本轮独立浏览器会话与内置浏览器测试页，停止 1420 端口 Vite，删除临时验证入口与截图；保留用户原有验证页和无关工作区改动。

### 10.7 Rust 测试调用点迁移补齐（2026-10-28）

- 审查发现来源联合迁移遗漏 5 处旧 struct 构造、2 处字段访问，以及 1 处工作区根初始化。修复前核心 prompt 集成测试编译报 `E0574`、`E0063`；桌面测试编译报 `E0574`、`E0615`、`E0609`，阻断 CI 的 workspace 全部目标检查。
- 将任务 prompt、定时任务、定时续聊和停止会话队列测试统一迁移为 `PromptWorkspaceFileRef::Workspace`，为只测试普通工作空间文件的根补充 `paths: None`；续聊接口测试改为断言完整引用集合，固定来源变体、project 与相对路径保留契约。
- 核心回归：`cargo test -p gold-band --test provider_prompt_bundle` 从编译失败转为 38 项通过。
- 桌面回归：`cargo test -p gold-band-desktop --bin gold-band-desktop workspace_file` 从测试目标编译失败转为 78 项通过、2 项按原有标记跳过；`cargo test -p gold-band-desktop --bin gold-band-desktop direct_conversation_run_vm_keeps_prompt_queue_on_stopped_leaf` 的停止会话队列回归 1 项通过。
- `cargo fmt --all -- --check` 与 `git diff --check` 通过；全局检索确认旧 struct 构造和直接字段访问已清除。桌面测试仍有 2 项既有未使用方法警告。
- 与 CI 相同的 `cargo clippy --workspace --all-targets -- -A clippy::all -D clippy::disallowed_methods` 通过，核心与桌面的所有目标均不再出现此次类型迁移导致的编译错误；输出仍包含既有未使用项警告。
- 过度设计与性能复核：仅补齐现有测试调用点和文档，没有新增运行时抽象、依赖、状态、缓存、I/O 或渲染成本。复用既有接口测试，无需新增专用检查框架。

### 10.8 受控交付附件预览再次引用（2026-10-29）

- 根因属于固定来源设计的实现遗漏：从消息引用打开的交付附件持有有效 `source`，但转换函数按 `scope=workspace` 过滤了运行目录内的外部附件；交付附件卡片打开的预览资源也未透传已有 manifest 来源。
- 修复前，两个路径分隔符的打开、再引用与提交接口用例均返回 `null`，真实文件面板 DOM 用例找不到引用按钮，附件卡片的预览映射用例缺失来源，共 4 项失败。转换函数改为依据已有固定来源保留附件引用，并从已解析路径取得展示文件名；附件面板补齐来源透传。没有固定来源的外部 locator 继续拒绝引用，提交仍携带完整 locator、changeSetId、attachmentId。
- 同一组失败用例修复后转绿；文件引用、来源根、附件面板、附件卡片菜单、bridge、Composer 与消息引用共 7 个测试文件、37 项通过。`npm run web:build` 的 TypeScript 检查及 Vite 生产构建通过，仍有既有大 chunk 与动态/静态混合导入提示。
- 内置浏览器使用真实消息引用 chip、打开 helper、文件预览、交付附件面板、Composer bridge 与上下文 chip，文件 API 使用隔离夹具。验证消息打开后再次引用、直接打开交付附件后引用、重复引用去重、完整嵌套来源提交，以及 390px 窄窗口长文件名与拉宽后状态保持；窄窗口文档宽度为 390px，无横向溢出。未重新打包 EXE，也未向远端 Agent 发送消息。
- 过度设计与性能复核：只补齐既有来源的转换和透传，沿用现有 identity、去重、共享上限与后端校验；没有新增依赖、持久字段、状态、缓存、请求、正文读取或订阅，转换成本仍与单个路径长度成正比。现有来源完整性和 Bug 验证规则已覆盖本次根因，无需新增经验规则。
- 验证页面与 viewport 已关闭/恢复，本次 1434 端口 Vite 和临时入口已清理；保留用户原有验证文件、无关工作区改动和验收截图。
