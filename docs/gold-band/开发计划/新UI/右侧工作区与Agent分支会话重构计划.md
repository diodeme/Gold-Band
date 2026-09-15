# 右侧工作区与 Agent 分支会话重构计划

## 1. 文档状态

- 状态：主体实现完成，正在执行最终回归与真实页面验收。
- 实现日期：2026-08-02。
- 范围：会话模式应用壳、右侧工作区、Agent 分支会话、会话投影与分页、实时事件路由和持久化。
- 本阶段首个右侧资源类型：Agent 只读会话。
- 后续可扩展资源：文件查看、Diff、产物、日志、并行会话等；本次不实现这些资源的具体内容。

## 2. 背景与根因

当前 ACP 主时间线将子 Agent 作为可折叠内容嵌套在父会话中。随着 Agent 数量、嵌套层级、工具调用和思考事件增多，该方案同时暴露了以下问题：

1. Agent 折叠层级过深，用户难以理解当前所处会话分支。
2. 展开 Agent 会把大量工具、思考、TODO 和嵌套 Agent DOM 挂载到主会话，导致滚动、展开和流式更新卡顿。
3. 主会话分页使用规范事件数量，而默认折叠后的可见内容可能只有少量 Agent 行，出现“页面没有可滚动内容但提示加载历史”的错误体验。
4. Agent 内部历史与主会话共用一个事件窗口，无法表达“主会话已完整、某个 Agent 详情尚未加载”的独立状态。
5. Agent 工具、权限和 TODO 的归属依赖前端对当前事件窗口重新分组；窗口截断后容易出现内容平铺、归属错误或状态不完整。
6. Agent 生命周期虽然已经有全会话投影，但其 transcript 仍与根会话事件混合持久化，导致实时路由和局部更新边界不清晰。

这不是单个折叠组件实现不完善，而是会话分支、应用布局和分页领域混在一起的结构性设计问题。因此不继续增加嵌套折叠补丁，改为应用级右侧工作区与独立 Agent 分支会话。

## 3. 设计目标

1. 应用形成稳定的左侧导航、中间主工作区、右侧辅助工作区三段式布局。
2. 主会话中的 Agent 只显示为可点击链接，不再内嵌 transcript。
3. Agent transcript 在右侧工作区中使用与主会话完全相同的消息渲染和实时更新能力。
4. Agent 会话只读，不提供自由输入、停止或继续入口；待决权限等阻塞性交互仍可操作。
5. 右侧工作区从第一版就使用通用、多 Tab 资源模型，不把状态写死为 Agent 专用侧栏。
6. 根会话和每个 Agent 都是统一的会话分支，复用同一套语义分页、滚动、贴底和消息组件。
7. 工具、思考等审计详情不参与会话历史分页，只在活动摘要展开后按需“显示更多”。
8. Agent 分支独立持久化并通过稳定内部 ID 关联，避免前端对 provider 私有字段做生命周期推断。
9. 只更新发生变化的会话分支；未激活 Tab 不挂载完整消息 DOM。
10. 保持原生滚动容器、有限事件窗口和真实 DOM 锚点，不引入会话 DOM 虚拟列表。

## 4. 非目标

本次不实现：

- 文件系统浏览器和文件编辑器。
- Diff、日志、产物等右侧资源的具体页面。
- 同一右侧工作区内同时平铺多个资源；本次一个工作区包含多个 Tab，但只显示一个激活 Tab。
- 子 Agent 自由输入、独立停止或独立继续。
- 从自然语言猜测 Agent 正在执行的意图。
- 把 Raw 协议帧作为普通会话消息展示。

## 5. 应用信息架构

### 5.1 三段式应用壳

```text
┌────────────┬────────────────────────────┬────────────────────────┐
│ 左侧导航    │ 中间主工作区                │ 右侧辅助工作区           │
│            │ 会话 / 工作流 / 上下文等     │ Agent / 文件 / Diff 等 Tab │
└────────────┴────────────────────────────┴────────────────────────┘
```

- 左侧导航继续负责工作空间、会话列表和一级页面入口。
- 中间区域始终是当前一级页面的主任务区域。
- 右侧区域是会话域内复用的辅助资源工作区，只在快速对话与 `ConversationRunPage` 可用；进入其他一级页面时隐藏入口和 Dock。
- 内部组件命名使用 `RightWorkspaceDock` 或 `AuxiliaryWorkspace`，避免与左侧导航 Sidebar 混淆。

### 5.2 右侧工作区 Tab

```text
┌ Agent A × ┬ Agent B × ┬ file.rs × ┐
├───────────────────────────────────┤
│ 当前激活资源内容                    │
└───────────────────────────────────┘
```

规则：

- 点击资源链接时，以稳定 `resourceKey` 查找已有 Tab；存在则激活，不重复创建。
- 关闭当前 Tab 后激活相邻 Tab。
- 关闭最后一个 Tab 后同步收起右侧工作区并清空激活态；需要使用空白入口页时通过顶栏右栏开关重新打开，工作区显隐仍由独立 `requestedOpen` 控制。
- Tab 过多时允许原生横向滚动且不压缩到不可读宽度；只有 Tab 条真实溢出时才显示小号完整 Tab 菜单，未溢出时隐藏该入口。Tab 条复用应用统一的 `gold-themed-scrollbar` 平台能力分支，不为局部高度切换浏览器滚动条渲染路径。Tab 之间保留轻量间距；激活项使用圆角弱底色和常显的低透明度关闭按钮，未激活项透明并在 hover 时反馈，不使用整格矩形、竖分隔线或底部选中横线。
- 多个 Tab 可以同时处于打开状态，但只挂载当前激活 Tab 的内容 DOM。
- 非激活 Agent Tab 只维护轻量状态和 attention 标记；激活时恢复分页窗口与滚动位置并补拉最新内容。
- 自动响应式收起只隐藏工作区，不关闭 Tab，不丢失 Tab 状态。

建议数据结构：

```ts
type RightWorkspaceResource =
  | {
      kind: "agent-transcript";
      key: string;
      title: string;
      locator: AgentTranscriptLocator;
    }
  | {
      kind: "file";
      key: string;
      title: string;
      path: string;
    };

interface RightWorkspaceState {
  tabs: RightWorkspaceResource[];
  activeTabKey: string | null;
  requestedOpen: boolean;
}
```

Tab 描述只保存资源定位与所属 `scopeKey`，不直接保存 timeline、文件内容等大对象。快速对话使用 draft scope，具体会话使用 project/task/run scope；轻量状态按 scope 进入 24 项运行期 LRU。宽度属于全局 UI preference，不放入 scope state；资源缓存、实时状态和 DOM 生命周期独立管理。

共享顶栏左侧按品牌、左侧导航开关排序，右侧工作区开关进入尾部操作区；Windows/Linux 中排在自定义窗口控制之前，macOS 中通过正常 flex 流停在右端并保留左侧原生 traffic lights 安全区，不使用绝对定位。顶栏统一使用 36px 紧凑高度，品牌图标容器为 24×36px，应用标题使用独立的 16px/700 字重，不复用全局映射为 520 的 `font-bold`；两个开关复用 shadcn `Button`，继续使用 28px 点击区、14px 图标和低对比打开态。Windows/Linux 窗口控制保留既有横向点击宽度并填满顶栏高度。右栏开关只在快速对话与会话详情展示，可在没有 Tab 时打开空白入口页。Tab 集合和激活态只在应用运行期记忆；像素宽度单独跨重启持久化。

### 5.3 会话 scope 与 LRU

- `draft:<projectId>` 表示尚未绑定具体会话的快速对话工作区；`conversation:<projectId>:<taskId>:<runId>` 表示具体会话工作区。
- 创建会话时将 draft 的 `requestedOpen` 迁移给新 scope，但不迁移资源 Tab；切换现有会话时直接恢复目标 scope，未命中或已淘汰时使用默认收起状态。
- `ConversationWorkspaceStore` 最多保留 24 个有状态 scope。访问顺序只由用户进入、打开、激活、关闭等工作区动作更新；后台 ACP streaming 不改变顺序。
- 删除会话或移除项目时同步删除匹配 scope。进程退出后轻量 LRU 清空，不把临时 Tab 持久化为用户配置。
- ACP Session VM、事件窗口和 branch view state 合并为同一 `AcpCachedResource`，统一限制 12 个 resource key；淘汰一个 key 时三类重对象一起释放。

## 6. 面板宽度与窗口响应式

### 6.1 组件选择

- 优先引入 shadcn `Resizable` copy-in 组件，底层使用成熟的 `react-resizable-panels`。
- 左侧导航、中间主工作区、右侧工作区进入统一水平 Panel Group。
- 不继续扩展当前应用壳中手写的 `mousemove/mouseup` 拖拽算法。
- shadcn Sheet 只用于紧凑宽度下的右侧资源覆盖模式，不作为常驻 Dock。
- 紧凑 Sheet 打开时把初始焦点放到 dialog 内容容器，避免空入口页把唯一的关闭按钮自动呈现为选中；关闭按钮保留键盘 `focus-visible`，但不使用 `data-state=open` 背景。
- 拖拽命中区域保持足够宽，但可见边界只使用低对比 1px 分隔，不显示粗色带。

### 6.2 页面布局配置

页面布局与原生窗口下限统一由 `configs/app-config.toml` 提供，Rust 解析、校验并通过 `AppConfigVm.workspaceLayout` 下发；前端只负责把当前页面映射到 profile。不得在页面组件、`tauri.conf.json` 或渠道 overlay 中散落第二套尺寸常量。

```toml
[workspaceLayout]
shellMinWidth = 480
shellMinHeight = 680

[workspaceLayout.conversation]
centerMinWidth = 360
centerAutoCollapseWidth = 420
windowMinWidth = 480

[workspaceLayout.contextCards]
centerMinWidth = 520
centerAutoCollapseWidth = 520
windowMinWidth = 520

[workspaceLayout.workflowCanvas]
centerMinWidth = 640
centerAutoCollapseWidth = 640
windowMinWidth = 640

[workspaceLayout.settings]
centerMinWidth = 480
centerAutoCollapseWidth = 480
windowMinWidth = 480
```

`centerMinWidth` 是 Resizable 和右栏压缩使用的内容硬下限；`centerAutoCollapseWidth` 是没有右栏时决定左栏何时自动收起的舒适宽度；`windowMinWidth` 是当前页面允许的原生窗口下限。会话内容允许压到 360px，但应用 chrome 下限仍为 480px；卡片、画布和设置按各自内容密度声明窗口下限。Rust 对非正值以及小于硬下限的阈值进行归一化，前端不得重新猜测配置。`scripts/channel-config.mjs` 生成的 default/wb overlay 继续完整继承基础 window config，仅覆盖渠道属性。

页面切换先计算目标 `LogicalSize(windowMinWidth, shellMinHeight)`，只有目标与已应用约束数值不同时才允许调用宿主 API。普通窗口先调用 `setMinSize`；若当前逻辑宽度低于新页面下限，再调用 `setSize` 扩宽。最大化窗口不得调用 `setMinSize/setSize`，只把最新目标记为 pending；用户恢复普通窗口后由 resize 生命周期在同一串行队列中应用 pending。进入更窄页面只降低约束，不主动缩小窗口。初始窗口保持隐藏，当前页面最小尺寸与主题 surface 同步完成后才调用 `show()`；连续导航必须保证最终页面配置最后生效。

### 6.3 横向收缩顺序

窗口横向缩小时：

1. 右侧工作区先收缩到自己的最小宽度。
2. 中间区域即将低于当前页面 `centerMinWidth` 时，自动收起左侧导航。
3. 继续缩小且中间区域再次接近最小宽度时，自动收起右侧工作区。
4. 剩余宽度全部交给中间区域。
5. 达到当前页面 `windowMinWidth` 后停止缩小。

窗口变宽时按相反顺序恢复：

1. 先恢复右侧工作区。
2. 再恢复左侧导航。

临界判断必须使用用户当前持久化的左栏宽度，而不是左栏最小宽度；否则中间区域会在达到 profile 最小值后仍被继续压缩。判断提供 48px 迟滞区间，避免窗口拖动时左右区域反复闪烁。手动折叠状态与自动折叠状态分别建模；自动恢复不得覆盖用户主动关闭右侧工作区的意图。

### 6.4 手动拖拽规则

- 用户拖动右侧分隔线时，左侧导航不自动关闭。
- 右侧宽度在统一最小值、最大值之间变化。
- 中间区域达到当前页面最小宽度后，继续拖动不再扩大右侧区域。
- 窗口边缘拖动和 Panel 分隔线拖动都必须逐帧改变真实尺寸；禁止 debounce 成“停止拖动后才切换尺寸”。连续像素变化交给 WebView flex layout 与 `react-resizable-panels`，React 只提交跨折叠阈值后的离散呈现状态。
- 左右宽度分别保存到会话 UI preference，不能只保存到临时组件 state。保存统一使用 `ResizablePanelGroup.onLayoutChanged` 的 `isUserInteraction=true` 完成事件，不使用连续 `onResize` 定时器推测拖动结束。
- sidebar VM 异步加载偏好后必须 hydrate 已挂载的 `RightWorkspaceProvider`；fallback 初值不能永久占据 reducer，也不能在首次打开 Agent 时反写覆盖持久化宽度。
- 拖动期间避免执行 timeline 重建、Markdown 重解析或面板内容重排。

### 6.5 紧凑窗口访问

响应式自动收起右侧工作区后，Agent 链接仍必须可用：

- 用户显式点击 Agent 时，使用同一资源内容在右侧 Sheet 中覆盖展示。
- Sheet 与 Dock 共用 Tab state 和内容组件，不维护第二套 Agent 页面。
- 窗口恢复足够宽度后可切回 docked 模式，保留当前 Tab 和滚动状态。

## 7. 主会话中的 Agent 链接

主会话不再渲染 `ChildAgentGroupCard` 的嵌套内容，只渲染轻量 `AgentLinkRow`：

```text
子 Agent  调查 ACP Rust 后端
运行中 · 调用了 24 个工具 · Read 涉及 11 个文件                 →
```

规则：

- 不提供 Collapsible，不在根会话挂载 Agent transcript。
- 展示名称、描述、结构化状态和 ACP 已确认的客观统计。
- 子 Agent 链接复用 Assistant 消息的共享 Agent 头像和用户头像 preference；嵌套 Agent 继承当前执行者身份，未配置头像时由同一组件提供 Bot fallback，不维护链接专用图标映射。
- 不猜测“正在运行测试”“正在搜索生命周期”等自然语言意图。
- 点击后在右侧工作区打开或激活对应 Agent 会话。
- 嵌套 Agent 在父 Agent 会话中继续使用相同链接组件。
- Agent 最终正式文字只在对应 Agent 会话中展示，不复制到父会话。
- Agent 等待权限时，链接和对应 Tab 显示 attention 状态。
- 同一个 Agent 的链接状态原位更新，不因 streaming update 创建新行。

## 8. Agent 只读会话

### 8.1 渲染复用

Agent 右侧会话与根会话复用同一个消息流实现，不复制 CSS 或重新实现工具、Markdown、活动摘要和分页。

目标拆分：

```text
ACPChatDialog
├─ ConversationHeader
├─ ConversationViewport       主会话与 Agent 共用
├─ InterventionLayer          权限、提问
└─ ConversationComposer       仅根会话使用

AgentConversationPanel
├─ AgentTabHeader
├─ ConversationViewport
└─ InterventionLayer
```

### 8.2 只读边界

Agent 会话不展示：

- 自由输入框。
- 模型与权限模式切换。
- 停止、继续、重试入口。

Agent 会话仍允许：

- 展开活动摘要和单条工具详情。
- 查看 Agent Prompt、TODO、正式文字和嵌套 Agent 链接。
- 响应当前待决 permission 或 elicitation，避免父会话因只读边界失去解阻入口。
- 查看状态、耗时、工具数量和文件读写统计。

权限响应继续作用于根 ACP session locator 与规范 request ID，不新建 Agent 专用权限协议。

### 8.3 停止与继续

- 停止、继续由根会话和统一 runtime lifecycle 控制。
- 根会话停止后，当时仍运行的 Agent 分支收敛为 interrupted。
- Agent Tab 保留历史，只读展示终态。
- 根会话继续后产生新的 attempt 或新的 Agent execution，不把新内容写入旧 Agent 分支。
- 如果未来 ACP 提供规范的单 Agent cancel 能力，再在领域接口层新增，不从当前 provider 私有行为猜测。

## 9. 统一会话分支模型

根会话与 Agent 会话不是两套数据结构，而是同一个 `ConversationBranch` 的不同实例：

```ts
type ConversationBranchId = string;

interface ConversationBranchLocator {
  projectId: string;
  taskId: string;
  runId: string;
  roundId: string;
  nodeId: string;
  attemptId: string;
  branchId: ConversationBranchId;
}

interface ConversationBranchVm {
  locator: ConversationBranchLocator;
  parentBranchId: ConversationBranchId | null;
  readOnly: boolean;
  status: string;
  page: ConversationBranchPageVm;
}
```

- 根分支使用稳定的 root branch ID。
- 每个 Agent execution 获得 Gold Band 生成的稳定 `AgentExecutionId`，并作为 branch ID。
- Agent ID 不直接使用 provider `toolCallId` 作为磁盘目录名。
- `parentBranchId` 表达嵌套关系，目录结构不递归嵌套。
- 根分支 `sessionElapsedSeconds` 是根 ACP attempt 的墙钟时间，不聚合 Agent duration；非根分支耗时由该 Agent execution 自身的 `startedAt` 与 `updatedAt` 得出。
- 根分支 `usage` 是 provider 对整个 attempt 报告的累计值。当前 Claude Agent ACP 会在同步根 turn usage 中包含嵌套 Agent 模型调用；Gold Band 不再逐 transcript 重复求和。非根分支没有 provider 独立 usage 时返回空值，不用推测值填充 UI。
- Todo 与直属子 Agent 必须由当前 branch timeline 和 Agent index 的 parent relation 投影；根、当前 Agent、兄弟 Agent 之间不得共享任务列表或会话统计。
- Claude `_meta.claudeCode.subagent/toolName/parentToolUseId` 只在 ACP 适配边界转换为统一 Agent transcript metadata；前端和持久化模型只消费内部字段。
- 将来 ACP 标准提供等价字段时，只替换适配层，不改变 UI 与存储领域模型。

## 10. Agent transcript 持久化

建议 attempt 目录结构：

```text
attempt-001/
├─ acp.raw.jsonl
├─ acp.timeline.jsonl
├─ acp.snapshot.json
├─ acp.agents.jsonl
└─ agents/
   ├─ agent-01/
   │  ├─ timeline.jsonl
   │  └─ snapshot.json
   ├─ agent-02/
   │  ├─ timeline.jsonl
   │  └─ snapshot.json
   └─ agent-03/
      ├─ timeline.jsonl
      └─ snapshot.json
```

职责：

- `acp.raw.jsonl`：完整协议排障事实源。
- 根 `acp.timeline.jsonl`：根会话语义事件与 Agent 链接事件。
- `acp.agents.jsonl`：Agent execution 生命周期和关系索引。
- `agents/<agentExecutionId>/timeline.jsonl`：该 Agent 分支自己的规范事件。
- `agents/<agentExecutionId>/snapshot.json`：该分支状态、统计和分页恢复锚点。

写入规则：

```text
ACP event
  → normalize agent relation
    → root scope  → root timeline
    → agent scope → corresponding Agent timeline
```

每个规范事件只写入所属会话分支。根会话中的 Agent link 是独立的聚合生命周期记录，不是把 Agent 工具和文字事件重复复制回根 timeline。

`acp.agents.jsonl` 至少记录：

- `agentExecutionId`
- `parentAgentExecutionId`
- `launchToolCallId`
- `sessionId`
- `status`
- `startedAt/endedAt`
- `eventCount/toolCallCount/readFileCount/writtenFileCount`
- `latestCursor`

开发阶段明确替换旧方案后，删除旧的 Agent launch anchor 注入和前端双消费路径。若必须保留现有测试会话，只允许提供一次性迁移，不保留长期双读兼容层。

## 11. 会话分页与活动详情

### 11.1 分页单位

会话分页基于稳定的语义块，不基于：

- 原始 ACP frame 数量。
- 规范 tool/thought chunk 数量。
- 当前 DOM 高度。
- 当前折叠或展开状态。

语义块包括：

- 用户正式消息。
- Assistant 正式消息。
- Agent 链接。
- 连续工具/思考聚合成的一个活动摘要。
- 当前待决交互。
- attempt 停止、继续、重试等边界。
- 上下文压缩等明确生命周期项。

```ts
interface ConversationBranchPageVm {
  branchId: ConversationBranchId;
  items: ConversationBlockVm[];
  oldestCursor: string | null;
  newestCursor: string | null;
  hasOlder: boolean;
  hasNewer: boolean;
}
```

根会话和 Agent 会话复用同一个 Page VM。不存在特殊的 `agentTranscriptPage`。

### 11.2 折叠内容不影响会话分页

- 一个活动摘要无论折叠还是展开，在会话分页中始终只算一个语义块。
- 一个 Agent link 在父分支中始终只算一个语义块。
- Agent 内部几百个工具事件不计入父分支 `hasOlder`。
- 展开活动或打开 Agent 不改变父分支 cursor。
- 如果根分支只有一条用户消息和两个 Agent link，且三者已经加载完整，根分支 `hasOlder=false`；不得因为 Agent 内部还有数百条事件显示“加载更早消息”。

### 11.3 活动详情“显示更多”

活动摘要展开后的审计详情使用独立、局部的增量加载能力，不称为会话分页：

```ts
interface ActivityDetailVm {
  items: ActivityAuditItemVm[];
  hasMoreEarlier: boolean;
  earlierCursor: string | null;
}
```

规则：

- 活动折叠时不加载、不构造、不解析详情。
- 首次展开只读取最近有限数量的审计行。
- 存在更早审计行时，只在活动内部显示“显示更早活动”。
- 点击后在当前活动内部追加，不影响会话分支 `hasOlder`。
- 单条工具 raw input/output 仍只在该工具再次展开时解析。
- 已决权限申请记录不进入活动审计展示；待决权限走实时 intervention。
- 如果产品将来决定会话内不提供完整审计，可删除 ActivityDetail 查询并把完整审计收敛到 Raw 页面，而不改变会话分页接口。

### 11.4 原生滚动与有限窗口

- 根分支和 Agent 分支继续使用 prompt-kit 原生滚动容器。
- 加载更早语义块时继续使用真实 DOM item 锚点补偿。
- 不使用动态高度 DOM 虚拟列表。
- 达到分支 buffer 上限时显示明确的“加载更早消息”；浏览旧窗口时提供“回到最新”。
- 页面不足一屏且确实存在更早语义块时可以自动回填；如果 `hasOlder=false`，不得根据原始事件总量继续请求。

## 12. 实时事件路由

不为每个 Tab 建立独立 Tauri 订阅。应用建立一个会话事件路由器：

```text
ACP live event(branchId)
  → ConversationEventRouter
    ├─ root branch store
    ├─ agent-01 branch store
    └─ agent-02 branch store
```

规则：

- 后端 live payload 携带规范 `branchId`。
- 根分支只接收根消息和 Agent link 生命周期/统计更新。
- 当前激活 Agent Tab 接收并合并完整分支流式事件。
- 非激活 Tab 不持续刷新完整 timeline DOM；只更新状态、attention 和 dirty revision。
- 激活 dirty Tab 时补拉该分支最新语义页并恢复滚动状态。
- 新事件只更新所属分支 store，不能触发整个 run VM 或所有 Tab 重建。
- permission、elicitation、terminal 和错误边界仍即时投递；普通 text/thought/tool streaming 使用既有 interaction-aware 合并节奏。
- 每个分支 timeline item 保持稳定 ID，未变化 item 复用对象引用。

## 13. 权限、TODO 与 attention 归属

- TODO 在规范化阶段按 branch ID 归属，只显示在对应根分支或 Agent 分支。
- Plan 归属使用内部 `planOwnership = branch | unscoped`。只有 provider relation 或现有内部 branch 定位能够证明归属时使用 `branch`；缺少 scope 的 session-wide plan 使用 `unscoped`，不得根据条目文本、Agent 名称或事件邻近猜测。
- 没有 Agent execution 的普通根会话可以展示 `unscoped` plan；存在任意 Agent execution 时根分支 fail-closed 隐藏 `unscoped` plan，避免 provider 聚合 Todo 平铺回主会话。
- 主会话不再通过文本内容去重来猜测哪些 TODO 属于嵌套 Agent。
- 待决权限保存其 branch ID，并向所有祖先 Agent link 投影 attention 状态。
- 根会话只显示 Agent link 的 attention，不把嵌套权限卡平铺到主消息流。
- 打开对应 Agent 会话后显示真实权限卡并允许决策。
- 权限决策完成后卡片退出待决状态，不在活动折叠区保留权限申请审计行。
- Agent 状态由统一 Agent execution lifecycle 管理，不用 launch tool 的原始 pending/completed 直接充当执行状态。

## 14. 性能约束

1. 收起或未激活的 Agent 不构造 timeline 详情。
2. 右侧只挂载一个激活 Tab 的 DOM。
3. 多 Tab 状态使用轻量描述符；timeline 缓存使用有限 LRU。
4. 工具输出只在单条展开后解析。
5. Activity 详情按需读取，避免一次读取几百 KB 或数 MB raw output。
6. 分隔线拖动和整窗 resize 的真实像素尺寸由原生窗口、浏览器 flex layout 与 `react-resizable-panels` 连续更新，不重建 timeline projection。
7. ResizeObserver 按 animation frame 采样宽度，`previousWidth` 保存在 ref；同一折叠阈值区间内无论经过多少像素都不得提交 React state，只有 `{left,right}` 改变时更新工作区呈现。
8. Agent summary 更新与 Agent transcript streaming 分层，主会话不消费子分支全部事件。
9. 分页仍基于 cursor 和有限窗口，不扫描前端完整历史计算 total。
10. 禁止为了多个已打开 Tab 将多个完整 ConversationViewport 长期隐藏挂载。

## 15. 实施阶段

### Phase 1：应用壳与右侧工作区基础

状态：已实现。

- 把 `ConversationShell` 提升为通用三段式 `WorkspaceShell`。
- 引入 shadcn Resizable copy-in 和 `react-resizable-panels`。
- 建立布局 profile、自动折叠状态机、迟滞和宽度 preference。
- 共享顶栏改为品牌在前、左右栏开关在后；右栏开关与资源 Tab 解耦，无资源时呈现空白入口页。
- 左右栏开关缩小按钮与图标尺寸并保留独立打开态；左栏入口留在品牌后，右栏入口通过尾部 flex 操作区靠近窗口右侧，降低识别成本并兼容 macOS 原生 traffic lights。
- 共享顶栏由 44px 收紧为 36px，品牌图标容器同步收紧，应用标题由 14px 提升为 16px 并使用独立 700 字重，绕开全局 520 `font-bold` 映射；帮助入口与左右栏开关统一落在 28px 操作节奏，Windows/Linux 自定义窗口控制只收紧纵向高度，保留既有横向命中宽度和原生拖拽/最大化生命周期。
- 修正右栏宽度恢复：Provider 接收异步 preference hydrate，拖动结束使用 group layout 用户事件持久化真实像素宽度。
- 统一 Shell 边界线：中间区顶部/左侧边界、右侧 Panel 顶边与右栏 separator 共享不透明 `workspace-divider` 主题 token；中间区和右侧区分别绘制同为 1 CSS px 的顶边，保证横向边界连续并与 separator 形成 T 形交点，消除半透明叠色与右侧缺线造成的交点色差/粗细错觉；不修改 Resizable 拖拽命中区和宽度热路径。
- 实现通用 Tab model、激活、关闭、去重、溢出列表。
- Tab 溢出入口改为由 `ResizeObserver + scrollWidth/clientWidth` 驱动，只在真实溢出时出现；横向滚动复用 Gold Band 主题滚动条。早期将该轨道独立压缩到 4px、显式恢复 WebKit 伪元素控制权的方案已被应用级互斥渲染策略替换，避免现代 Chromium/WebView2 在标准属性与 WebKit 伪元素之间混用。
- 会话页面 `centerMinWidth` 从设计初值 420px 校准到 360px；上下文卡片、工作流画布和设置 profile 保持不变。
- 使用静态 Agent resource 验证 docked/compact Sheet 两种模式。
- 在 `WorkspaceShell` 共同边界提供 shadcn `TooltipProvider`，确保中间会话、右侧 Dock 与紧凑 Sheet 复用含 Tooltip 的会话组件时拥有相同 UI 上下文；不在 Agent 面板内重复补 Provider。
- 右侧入口限制为快速对话与会话详情；引入 draft/conversation scope 和 24 项轻量 LRU，切换会话恢复各自 Tab、激活态与展开态，被淘汰会话回到默认收起状态。
- 将 ACP Session、有限事件窗口和 branch view state 的三套 12 项缓存改为单一 resource LRU，避免只淘汰部分数据。
- 修复响应式折叠不可达：Tauri 原生最小宽度从 1040px 调整为 profile 最大值 640px；布局阈值与右栏动态上限统一消费当前左栏宽度，保证窗口缩放真实触发 Dock → 自动折叠 → 同源 Sheet 路径。
- Dock/Sheet 切换时仅保留 Sheet 的退出动画外壳；compact 状态结束即卸载其中的 `RightWorkspaceDock`，避免动画期间双挂载 Agent 会话与重复订阅。
- 渠道 Tauri overlay 改为从基础 `tauri.conf.json` 完整继承主窗口配置，只覆盖渠道标题；删除 overlay 内独立的 1040px 最小宽度，避免浏览器状态机可达但 default/wb 客户端原生拖拽仍被旧约束截断。
- 布局 profile 增加 `centerAutoCollapseWidth`，将中间硬下限和无右栏时的左栏自动收起舒适宽度分开；会话使用 360px/420px，保证原生最小窗口下即使右栏从未打开，左栏也能进入自动折叠态。
- 优化窗口连续缩放热路径：删除 `availableWidth state -> autoCollapse state` 的每帧双提交，把 `previousWidth` 下沉到 ref；右栏使用 Resizable 原生 min/max 与中间 min 联合约束，不再每像素重算 max prop。左栏移除连续 `onResize` 持久化定时器，与右栏一起只在用户释放分隔线时保存；会话导航、置顶索引和右侧 Dock 增加稳定 memo/Set/Map 边界。单元契约以连续 100 个同区间像素样本固化零呈现更新，并保留跨阈值一次更新、迟滞及恢复顺序。
- 修复 Windows 最大化切页还原：窗口约束同步维护 `appliedMinimum + pending`，同尺寸 profile 不调用 Tauri；最大化期间延迟不同约束，恢复后应用最新 pending，避免 TAO `set_min_inner_size -> set_inner_size` 清除最大化状态。

### Phase 2：统一 Agent 分支领域模型

状态：已实现。

- 定义 `AgentExecutionId`、`ConversationBranchId`、branch locator 和 Agent index。
- 在 ACP 适配边界把 provider metadata 转为统一关系模型。
- 生命周期、TODO、权限和统计统一绑定 branch ID。
- 删除前端 provider 私有字段消费。

### Phase 3：分支持久化与查询接口

状态：已实现。

- 根 timeline 与 Agent timeline 分流写入。
- 增加 Agent index/snapshot。
- 增加按 branch cursor 查询语义页的接口。
- 增加按 activity cursor 查询审计详情的接口。
- 删除 Agent launch anchors 改写全局分页窗口的路径。

### Phase 4：会话渲染器拆分

状态：已实现。

- 从 `ACPChatDialog` 提取 `ConversationViewport`、`InterventionLayer` 和 composer。
- 实现 `AgentConversationPanel` 只读容器。
- 根会话和 Agent 分支共用消息、活动、工具、分页、贴底和 Markdown 实现。

### Phase 5：Agent 链接与实时路由

状态：已实现。

- 用 `AgentLinkRow` 替换嵌套 `ChildAgentGroupCard`。
- 建立应用级 ConversationEventRouter。
- 支持嵌套 Agent 从父 Agent Tab 打开新 Tab。
- 接入 attention、permission 和 terminal 状态。

### Phase 6：破坏式清理

状态：已实现。

- 删除嵌套 Agent Collapsible UI。
- 删除 `subAgentHistoryOutsideWindow` 和原始事件数提示。
- 删除根/Agent 共用事件窗口的旧分页逻辑。
- 删除旧 Agent launch anchor 注入和前端兼容消费。
- 删除已无调用方的状态、i18n 和测试 fixture。

### Phase 7：会话辅助资源迁移

状态：已实现。

- 扩展统一 `RightWorkspaceResource` 为 `workflow-view`、`workflow-edit`、`system-prompt`、`raw-frames`，所有描述符只保存当前 scope 与稳定 locator。
- “查看工作流 / 编辑工作流 / 修复工作流”从会话 Sheet 迁移为右侧 Tab；保存继续调用既有任务工作流接口并刷新 run snapshot。
- “系统提示 / 原始帧”从 ACP 内部 Dialog/画布切换迁移为 attempt-scoped 右侧 Tab；根会话与嵌套 Agent 使用同一资源协议。
- 右侧 Dock 只挂载激活资源，通过当前 conversation-run 注册的资源 renderer 读取实时 run 与保存控制器，拒绝把 Graph、prompt 或 raw page 写入 Tab/LRU。
- 工作流草稿使用独立 24 项运行期缓存；收起 Dock、切换 Tab 和切换会话后仍可恢复，主动关闭脏 Tab 需确认。
- 原始帧筛选区拆成“全宽搜索 + 横向可换行 Select 组”，按资源容器真实宽度自然降级，不再用过大的 container breakpoint 让筛选器整组竖排。
- 去掉会话标题栏“原始帧”入口与右侧激活 Tab 的重复选中态；右侧 Tab 是资源选中的唯一视觉来源，非工作区旧页面的真实 canvas 切换继续保留按钮 active 状态。
- 保留非会话旧页面的 ACP fallback 展示，但在存在 conversation workspace scope 时只能走右侧资源，不允许同一上下文同时存在两套消费路径。

## 16. 测试与验收

### 16.1 应用壳

- 三栏均打开时，拖动右栏不能把中间区域压到当前页面最小宽度以下。
- 会话中间区可继续收窄到 360px；同样窗口尺寸下，卡片与画布页面仍按各自更大 profile 提前停止拖动。
- Tab 未溢出时不显示完整 Tab 菜单；真实溢出后显示小号入口，关闭 Tab 消除溢出后入口同步隐藏。
- 激活 Tab 呈现圆角弱底色与常显关闭按钮，不显示整格填充、竖分隔线或底部选中横线；横向滚动条与选中态不会叠成双线。
- 主会话与 Agent 会话中的子 Agent 链接都渲染共享 Agent 头像；自定义头像更新后链接同步变化，attention 状态只增加可见外圈。
- 缩小时先自动收起左栏，再收起右栏；放大时先恢复右栏，再恢复左栏。
- Tauri 主窗口可从三栏状态继续缩小到 640px；会话页在约 `sidebarWidth + 360 + 320` 时收左栏、低于 680px 时收右栏，不得在 1040px 提前停止。
- default 与 wb 渠道生成的最终 Tauri overlay，其主窗口尺寸、最小尺寸、可见性、透明度、阴影与背景属性必须和基础配置一致，只有标题允许因渠道变化。
- 最大化状态下从会话详情双向切换设置页，窗口必须持续保持最大化；切换到不同 `windowMinWidth` 的页面也不得调用 `setMinSize/setSize`，恢复普通窗口后才应用最新约束。
- 会话与设置使用相同 480×680 约束时，双向切换不得重复调用宿主窗口 mutation；接口级测试必须分别固化 unchanged、maximized deferred、restored applied 三条路径。
- 右栏关闭的会话页缩到约 `centerAutoCollapseWidth + sidebarWidth` 时自动收起左栏，并在反向放宽超过 48px 迟滞后恢复；该路径不得依赖右栏曾经打开。
- 临界宽度附近来回拖动不闪烁。
- 自动收起不关闭 Tab，手动关闭状态不会被自动恢复覆盖。
- 关闭最后一个 Tab 后右侧同步收起，标题栏右栏开关仍可重新展开空白入口；重启后 Tab 清空但宽度恢复。
- sidebar VM 晚于应用壳到达时，持久化宽度仍能覆盖 440px fallback；用户拖动结束只写一次真实像素宽度。
- 紧凑宽度点击 Agent 能以 Sheet 打开，恢复宽度后能回到 Dock。
- Sheet → Dock 退出动画期间只能存在一套 `RightWorkspaceDock` 内容，不能短暂双挂载 Agent transcript。
- 会话、上下文卡片、工作流画布使用各自容器宽度降级，不依赖整窗 breakpoint。
- 从资源模型打开 Agent Tab 后，其会话内容中的 Tooltip 可以直接挂载；异步加载完成不得因缺少 `TooltipProvider` 停留在“加载中”、清空 WebView 或触发未捕获异常。
- 快速对话与会话详情显示右栏入口；Agent 管理、上下文管理、运行模式管理和设置不显示入口或 Dock。
- A/B 会话切换只呈现各自 scope 的 Tab；返回 A 恢复 A，未命中或第 25 个 scope 淘汰后进入会话时右栏默认收起。
- LRU touch 只由用户访问和工作区操作触发，后台流式事件不能使非当前会话长期保热。
- draft 创建新会话时迁移展开意图但不迁移资源 Tab；删除会话、移除项目同步清理 scope。
- ACP 重资源缓存淘汰时，同一 key 的 Session、events 与 view state 必须同时不可恢复。
- 四类会话辅助入口打开后均产生当前 scope 的 locator-only Tab；Tab state 不包含 workflow JSON/Graph、system prompt 正文或 raw frame page。
- 查看 raw frame 或 system prompt 不改变主会话 canvas mode；从嵌套 Agent 打开时 locator 保留该 Agent 的 branch ID，返回原 Agent Tab 时不重新进入初始加载。
- 编辑工作流后收起并重开右栏仍恢复草稿；关闭脏 Tab 被 guard 拦截或经确认后丢弃，保存后 guard 自动解除且主 run snapshot 刷新。

### 16.2 会话与分页

- 500 个嵌套工具事件、2 个顶层 Agent 的根会话只显示用户消息和 2 个 Agent link，且 `hasOlder=false` 时不显示历史提示。
- Agent index 中有 2 个顶层、25 个嵌套 execution 时，根 projection 只返回 2 个顶层 Agent；任一 Agent branch 只返回 `parentAgentExecutionId` 指向自身的直属孩子。
- 活动中 100 个工具和思考事件只形成一个会话语义块。
- 折叠、展开活动不改变会话 cursor 和 `hasOlder`。
- 展开活动只加载最近审计行，并可在内部“显示更早活动”。
- Agent 分支的语义分页与根分支使用同一套接口和组件。
- Agent link 只加载对应分支，不触发其他分支重新投影。

### 16.3 生命周期与交互

- 2026-09-07 Agent 自引用进度修复：工具更新先沿用稳定调用归属再选择 transcript；索引统一合并最早启动归属与最新执行证据。覆盖顶层和嵌套 Agent 的自身进度、取消后继续、索引与完整重建结果一致性。
- 红测证据：运行时合并错误返回 Agent 自身 branch；索引错误返回自身 parent；前端摘要缺失错误返回 queued，DOM 随父会话状态显示“等待执行／已中断／已完成”。修复后缺失摘要显示“状态未知”，已有终态仍优先，禁止恢复父会话状态兜底。
- 自评审：复用现有调用 identity、timeline 和 branch index，无新增依赖、持久字段、状态机或缓存；每次工具更新只合并常数级归属字段，索引查询不增加历史读取范围。
- 验收：`cargo test -p gold-band --lib acp::` 450 项通过、1 项原有忽略；前端 indexing、read-only-agent-panel、conversation-event-router 共 72 项通过；类型检查与生产构建通过。补充的 status-only 完成结果测试确认正文仍进入正确 Agent 分支。实际 task-015 / run-001 / phase-6-finish-after-capacity-blocker 落盘记录在临时副本上查询后得到 root parent、interrupted、原取消时间 `1788749938Z`，未修改用户日志。
- 浏览器验收：内置浏览器无可用连接，改用独立 agent-browser 会话挂载实际 ACPChatDialog；验证父会话 cancelled → running 后仍显示“已中断”，无摘要行显示“状态未知”。1440×900、640×800、重新拉宽和明暗主题下均无行溢出，测试页面、进程和临时副本在验收后清理。未替换当前运行的正式 EXE。

- Agent launch tool 已完成但分支仍在生成时，Agent 状态保持 running。
- Agent 只有 launch、尚无内容时显示 queued；产生工具或文字后进入 running。
- 根会话停止后所有活动 Agent 收敛为 interrupted。
- Agent 内权限申请使对应链接和 Tab 出现 attention；进入 Tab 后可以决策。
- 已决权限不出现在活动审计详情。
- TODO 只出现在所属分支，不平铺回根会话。
- 根会话耗时保持根 attempt 墙钟值，不因多个 Agent 并发或串行执行而求和；Agent 顶部耗时只属于该 Agent execution。
- 根 Token 展示 provider-reported attempt 累计 usage；Claude Agent ACP 的嵌套 Agent usage 计入根值，Agent 分支不展示无法确认的独立 Token。
- 存在 Agent execution 时，根 timeline 中没有 relation 的 session-wide plan 不生成 Todo；明确 scoped 的 Agent plan 仍在 Agent Tab 展示；没有 Agent execution 的普通根 plan 仍展示。
- Todo 归属测试必须证明实现不读取条目自然语言进行 Agent 匹配。

### 16.4 实时和性能

- 更新 Agent B 时，Agent A 和根历史项保持对象引用。
- 非激活 Tab 不挂载 ConversationViewport DOM。
- 非激活 Agent streaming 不持续驱动完整 Tab React render。
- 切换 Tab 能从 `acpChatResourceCacheSessionCount` 控制的有限 LRU（默认最多 8 个 branch key）同步恢复完整 Session VM、滚动位置、分页窗口、正文 hydrate 标记和贴底状态；后台刷新期间不得重新展示加载壳。
- canonical 非根分支一旦带有 `branchExecution` 即结束首次加载；不能因缺少根会话 metadata 或状态为 `interrupted` 进入 `missingAcpSessionRetryDelay` 退避链。
- ACP session 查询支持可选 `traceId`，前端 effect/request 与 Rust command/view-model 各阶段使用同一 ID 记录耗时。调试开关为 `goldBand.debug.acpTiming=1`；关闭时不传 trace、不输出逐请求日志。
- 工具大输出在活动和工具折叠时不解析。
- 原生滚动分页锚点在主会话和 Agent 会话中均稳定，无跳动和错位。

### 16.5 持久化

- 根事件只进入根 timeline，Agent 事件只进入所属 Agent timeline。
- Agent index 能恢复父子层级、状态和统计。
- 应用重启后打开 Agent link 能恢复历史并继续接收实时更新。
- 快速对话 draft、定时创建与会话详情共用全局右栏宽度偏好；任一页面完成 separator 拖拽后，切换页面、折叠重开或重启应用都恢复同一有界像素宽度，不得回退到 Panel 首次注册尺寸或最小宽度。实现使用 `react-resizable-panels` 的 imperative `resize()` 在离散展示/hydrate 时恢复，pointermove 热路径不写 React state 或存储。
- 文件名和目录只使用 Gold Band 稳定 ID，不使用未经处理的 provider ID。
- storage query 使用结构化错误码，不返回后端对客文案。

## 2026-09-07 过程列表两层展开与正文恢复

- 根因归类：保护展开阅读和有界历史窗口的设计合理，但消费契约混淆。`f5a3f86f` 引入展开暂停跟随，`f8182504` 将不跟随当作历史正文合并闸门，`94ef9471` 扩散到快照与重入；`1f162731` 的 120px 按钮门槛又隐藏了物理底部的数据恢复入口。task-004 的纯文本 DOM 复现证明无需图片即可触发，不能归因于图片解码。task-343 的 active writer 恢复错误、task-015 的展开诊断仍是独立事项。
- 实现：整个过程列表首批详情就绪后定位到自身底部一次并暂停跟随；单条工具/思考在点击处向下展开并暂停跟随。复用共享 token，手动滚动取消恢复资格，最后一个 token 结束后才恢复原跟随意图，移除 resize-at-bottom 的隐式恢复。
- 正文：临时展开不再阻断容量内的 live 投影；容量不足时保留阅读窗口和 newer edge，收起、回最新及重入复用有界 canonical handoff。后台恢复不得抢走展开位置；数据恢复入口不受 120px 限制。临时 disclosure 不写成持久的手动阅读意图。
- 失败证据：修改实现前，纯文本展开测试两例均缺失后续回复，多 token 测试在几何底部提前恢复；异步详情测试保持旧 scrollTop 而未定位；单条工具/思考测试在展开后仍为 following。均已使用同一测试验证转绿。
- 验收完成：9 个相关测试文件共 212 项通过，覆盖收起/未收起重入、容量边界、恢复失败重试、异步详情取消、单条工具/思考原地展开、无 scroll 的内容增长恢复入口，以及 owner/generation/replay/分页回归。类型检查、主题生成和 Vite 生产构建通过；构建保留现有大 chunk 和混合导入告警。
- 浏览器证据：使用真实 `ACPMessageList + ConversationViewport` 临时页面验证 30 项过程与长输出，外层展开底部按钮到 composer 顶部误差小于 1px；单条展开标题在桌面位置变化约 0.64px，在窄窗口保持 768.31px。追加回复后正文存在、scrollTop 不变、following=false；直接收起恢复 following，主动上滚再收起仍为 false。420px 内容区及恢复 1100px 均无横向溢出。桌面截图与窄内容区折叠截图已检查；部分尺寸下截图接口超时，展开位置与溢出由真实 DOM 几何补充验证。未重新打包或重放 EXE 原现场。临时页面与本次开发服务在验收后清理。
- 过度设计与性能评审：无新依赖、持久字段或缓存；新增共享 hook 只统一两类单条披露的 token 释放。主窗口默认 96×3、详情 40×3 上限不变，复用 latest-wins 批量发布和 single-flight；单次定位只读取目标/视口几何，折叠时不解析详情，不增加全量历史读取或轮询。

## 2026-09-07 展开溢出定位与收起跟随契约细化

- 本节替代上一节“无条件定位自身底部、收起恢复原跟随意图”的交互；上一节的正文合并、身份与容量保护继续有效。
- 根因：原 `3d16e770` 按当时约定实现强制对齐，未区分放得下与正向溢出；恢复旧 follow 意图也会跨过展开期间的新回复，属于需要替换的交互设计。footer 测量仅包含 block height、遗漏 absolute 角标，属于正确共享布局设计的实现缺口。第三方 `use-stick-to-bottom` 在内容收缩且接近底部时自行恢复锁定，必须由拥有权威意图的包装层约束，不能只修改业务页收起按钮。
- 实现：历史/最新过程列表均先原地向下展开，首批详情就绪只补偿超出共享阅读底边的正向距离一次；单条工具/思考维持标题原位展开。收起移除额外 scrollIntoView，最后一个 token 结束后的下一帧仅在物理底部且无 newer/page/recovery 时恢复 follow；主动滚动取消待恢复帧。库的 resize 收缩不得绕过暂停意图。
- footer：复用一个 ResizeObserver 观察 footer 与稳定角标定位层，按真实最上边缘更新底部留白；任务/队列变化仍实测，不保留最大高度、不触发展开再次定位。角标父层宽度仍与 composer rail 一致，保留现有响应式布局。
- 同帧时序：observer 与展开定位共享测量/留白提交函数；定位前同步提交实际 footer 占位，确保滚动范围已经包含本帧增高的队列。浏览器红测只更新目标边界时差约 100px，DOM 回归中目标 scrollTop=320 被旧范围截到 200；修复后同一测试转绿，浏览器列表末尾与角标顶边误差约 1px，following=false。
- 重入：展开前 follow 意图只用于未收起离开后的缓存恢复；已知真实 session 的缓存仍有 newer edge 时，在会话 reset 完成后显式唤醒现有 canonical coordinator，不再依赖已卸载 disclosure 的收起回调。无 sessionId 的 pending timeline 不触发该恢复。
- 红测证据：放得下的展开从 scrollTop=100 被拉到 0；新回复到达后收起从 139 跳到 199；角标 24px 未计入 96px footer；有界历史窗口收起意外发起追新请求。补充测试证明收缩后库会重新锁定，以及收起待恢复帧不能被向下输入取消。均先观察失败后修改对应实现。
- 过度设计/性能自评审：复用 prompt-kit 与 use-stick-to-bottom，无新依赖、持久字段、缓存或业务状态机。新增资格回调仅读取现有 refs；footer 每帧最多测量其自身与一个角标 wrapper，O(1) 局部 CSS 写入；只在 footer 挂载时查找标记节点，不扫描消息。96×3 主窗口、40×3 详情窗口及 single-flight 容量不变。
- 验收完成：14 个相关测试文件共 278 项通过，覆盖多层 token、收起帧用户输入取消、最新/历史窗口、容量边界与重入恢复、异步详情、同帧 footer 更新和浏览器滚动范围限制，以及角标响应式、composer 渲染隔离、视觉与滚动条契约。TypeScript、主题生成与 Vite 生产构建通过；保留现有混合导入与大 chunk 告警。
- 浏览器证据：真实 ACPMessageList、ConversationViewport、AcpUsagePanel 组合中，短历史展开 scrollTop=0 不变；桌面长列表仅补偿约 408px，末尾到角标误差小于 1px；单条工具标题桌面 162.58px、窄内容区 688.21px 均保持原位。追加较长回复再收起维持 scrollTop=407.62 与 following=false；内容收缩自然到达底部后 following=true。420px 内容区及重新拉宽无横向溢出，队列增高仅更新占位，不再次定位。最后使用同帧队列增高/展开的独立共享组件场景复验，列表底部 227.09px、角标顶部 226.01px，following=false。已检查桌面/窄布局截图，全部临时文件、标签页、viewport override 和开发进程已清理；未重新打包或重放正式 EXE。
- 完成自评审：共享测量/留白提交函数消除尺寸投影与一次性定位之间的时序差异，未新增 observer、持续滚动任务、全量历史读取、额外缓存或持久状态；接口测试和真实浏览器证据共同覆盖根因。规则目录已有 canonical state、局部几何和阅读锚点约束，本次不新增经验规则。

## 2026-09-08 流式 Activity 详情请求与首批展开定位

- 根因回溯：`94ef9471` 为恢复和会话隔离引入详情 request scope 校验，将 activity end revision 也作为整页失效条件；随后展开定位等待详情就绪，使持续流式下的反复作废表现为一直不定位。属于正确隔离设计的实现边界不完整。另一个契约缺口是后端按 startedSeq 选项并返回最新版本，前端却用 endedSeq 限定请求范围，拒绝了合法更新。
- 实现：同 owner/session/generation 的页面即使落后于 live 范围仍可接纳，按现有事件版本合并，保留较新工具状态；不同 generation 重新创建详情窗口。详情窗口区分首批可阅读与当前范围已加载，前者不随 live 更新回退；一次展开定位只等待前者。后续缺口仍由既有一个 in-flight 和一个合并 trailing 请求补齐，不新增轮询。首次局部 live 内容可见时仍显示 loading，刷新保留分页入口与阅读窗口。
- 红测证据：流式范围由 109 前进至 111 后，初始 40 条详情全部被拒绝，DOM 行数为 0；起点 109、结束位置 115 的合法版本同样不显示。修复过程中补充测试发现摘要同步会覆盖首批页的 earlierCursor，导致“显示更早”活动入口消失，已改为保留已加载窗口游标。最终有界合并回归还复现了窗口起点已变为 121、游标仍为 before-201 的问题；现由同一同步入口统一裁剪并更新为 rev:121。
- 验收：同一失败测试转绿；7 个相关测试文件 226 项通过，覆盖首批接纳、一次溢出定位、放得下不滚动、用户取消、footer 边界、跨会话/generation 拒绝、范围校验、较新工具状态保护、分页容量和会话恢复；补充最终裁剪游标用例后，详情测试文件全量 33 项通过。TypeScript、主题生成和 Vite 生产构建通过，保留现有混合导入及大 chunk 告警。
- 浏览器：iab 不可用后使用已连接 Chrome，真实 ACPMessageList + ConversationViewport 配合可控延迟接口。40 条首批详情在流式范围前进后显示，桌面列表底部与角标顶边误差小于 1px；随后详情刷新增加行数，scrollTop 保持 668、following=false。420px 内容区首批同样定位到角标上沿，误差小于 1px；直接点击单条长输出标题维持 601.89px，重新拉宽无横向溢出。浏览器只模拟接口延迟与流式竞态，未重放正式 EXE 原会话。
- 完成自评审：复用 prompt-kit、有界详情窗口 40×3、事件 reducer 和 single-flight，无新依赖、持久字段、缓存或并发队列；只增加详情窗口自身的首批就绪标记，表达新鲜度不能替代的阅读生命周期。合并限定于已有窗口与单页，不增加全量加载或扩大订阅；后端原查询耗时未改变，不宣称消除了首次 I/O 延迟。规则目录已有生命周期、单调合并与分页锚点约束，不新增经验规则。

## 2026-09-08 计时回放水位溯源与修复（已验证）

- 授权范围：完成溯源、最小失败复现后，经用户确认实施根因修复。复用 session-scroll-pagination 的消息身份与阅读恢复检查，以及现有 live/session timing 接口；不引入新库、缓存、队列或运行状态。
- 引入链路：`3ec88f06`（2026-07-03，stabilize live session timing）增加约每秒的临时计时事件，ID 包含时间、seq 使用执行实例当前 seq，目的是在工具运行和等待阶段持续更新计时。`280ad51e`（2026-08-19，harden prompt turn admission）以 durable revision 建立有界回放的丢失确认，计时无 revision，不要求 durable catch-up。`94ef9471`（2026-08-31，stabilize timeline recovery and session isolation）为未落盘正文补独立 sequence fence，并以 canonical newestSeq 确认；同次提交明确把原测试“does not make transient timing updates part of durable catch-up”替换为“uses sequence coverage to recover an oversized transient timing update”，将纯展示计时也纳入恢复要求。
- 根因分类：恢复契约的数据分类缺陷。暂未落盘但会进入消息历史的内容与永不作为正文落盘的计时都没有 revision，不能仅凭该字段缺失赋予相同的消息追齐要求。有界缓存和会话隔离初衷正确；子分支使用独立正文与 replay key，但主分支计时沿用全局 seq，可把子分支进度间接带入主分支 sequence fence。不是最近 Activity 展开提交 `8319e92f` 引入。
- 接口复现：先确认主正文 seq=118，再向 child 分支发布 seq=388，随后向 root 发布正常大小、独立时间 ID、无 revision 的计时事件。容量来自 `CONVERSATION_EVENT_REPLAY_LIMITS.eventsPerBranch=64`，64 条对照组 ACK 成功；65 条触发淘汰，root 没有 child 正文、loss revision=0，但 ACK(coveredSeq=118) 返回 false。失败位置是明确布尔断言，不是等待超时。
- DOM 复现：真实 ACPChatDialog 在相同回放条件下重新进入，getAcpSession 始终成功返回已完整覆盖的 root 正文，子正文不显示；等待生产有界恢复预算结束后点击“回到最新”，确认新查询发生且按钮结束 loading，最终按钮仍存在。失败位置是 DOM 按钮应移除断言。测试等待预算只用于观察恢复完成，不参与构造竞态；与接口无等待对照共同证明原因。
- 修复前红测：`node node_modules/vitest/vitest.mjs run --config web/vitest.config.ts web/tests/conversation-event-router.test.ts web/tests/acp-session-reentry-reconciliation.test.tsx -t "child-advanced"`，结果 1 通过、2 预期验收失败。修复后保留 ACK 成功及最终按钮移除断言转绿；重入已能自动清除按钮时无需再点击。
- 实现：Router 在 session/generation 校验及换代清理后，让 timingUpdate 继续 live 分发，但跳过正文回放存储、head 推进、payload 计量及淘汰。审视既有消费者后不增加“最新计时缓存”：活动页面已有 live 分发，重入已有 canonical session timing，避免复制权威状态。其他事件保留原 revision/sequence fence 行为。
- 验证：Router、订阅、会话重入、贴底、Activity 详情加载 5 个测试文件共 178 项通过；TypeScript 构建类型检查及 Vite 生产构建通过（保留现有静态/动态混合导入及大 chunk 警告）。新增保护用例覆盖连续计时不挤掉正文、计划、用量和权限事件，以及这些持久事件真正超限时仍建立 revision loss。
- 浏览器：iab 不可用后使用 Chrome，在模拟运行时数据的真实 ACPChatDialog 中验证 24 条长消息、主正文 seq=24/计时 seq=388、累计 195 次计时。实时累计由 1m5s 更新到 2m10s；上滑出现按钮、点击后消失；离开期间再推进计时，重入显示 3m15s 且按钮消失。宽窗口及 420px 容器截图可读，窄容器距物理底部 1px。临时入口、页面和开发服务在验收后清理。
- 证据范围：证明这条水位链路可导致按钮无法消失；未在正式 EXE 重放 task-021 原现场，也未证明截图重入锚点的具体像素位置由同一原因唯一决定。原现场没有前端 replay 淘汰/点击 trace，不能把代码复现当成该次点击的完整审计。
- 性能与过度设计评审：生产改动限于 Router 6 行，复用现有数据边界，无新增依赖、状态、I/O、全量扫描或缓存；减少每次计时的 replay payload 计量、缓存占用与无效恢复请求，实时展示频率不变。容量仍使用现有 64 条配置，回归覆盖临界 64/65 条；无需另建计时状态机或 benchmark。未提交。

## 2026-09-09 原始帧顶部工具栏固定

- 根因：`494eed351` 将原始帧迁入右侧资源时，宿主使用整页 `overflow-y-auto`，共享查看器未区分工具栏与列表视口，属于正确资源化设计下的布局实现缺陷。
- 修复：共享 RawFrameViewer 负责固定工具栏和唯一帧列表滚动区；右侧资源与独立 Raw 画布宿主只约束高度。复用现有 shadcn/ui、Tailwind 和主题滚动条。
- 修复前证据：`raw-frame-viewer-layout.test.tsx` 两项 DOM 测试分别因缺少列表独立视口、资源宿主仍整页滚动而失败；内置浏览器加载 100 条长帧后滚动 1440px，搜索栏顶部从 24.8px 移至 -1415.2px。
- 回归验收：真实查看器的滚动边界、100 条帧渲染、搜索和翻页参数，以及资源宿主布局与首次查询次数均通过；`raw-frame-viewer-layout`、`gold-themed-scrollbar`、`responsive-layout-contract` 共 22 项通过。`npm run web:build` 的 TypeScript 检查和生产构建通过，保留现有混合导入及大 chunk 警告。
- 浏览器验收：内置 iab 中使用真实资源面板、模拟分页接口和 240 条长帧数据（当前页 100 条），1280×720 下列表滚动 1440px 后搜索栏顶部仍为 24.8px，资源宿主滚动为 0；直接翻到第二页及搜索指定帧成功。420×740 下工具栏自然换行，展开长帧并滚动后控件仍固定、横向溢出为 0；清空搜索并重新拉宽后恢复 100 条列表和正常布局。未执行 EXE 后端联调；临时验证入口与服务在验收后清理。
- 性能与过度设计评审：仅调整 CSS 布局与 DOM 分区，沿用每页 50/100/200 条与按需展开正文；无新增依赖、状态、缓存、监听、I/O 或扫描，查询与渲染范围不变，无须新增 benchmark。

## 17. 文档同步要求

实现阶段必须同步维护：

- `docs/gold-band/产品设计文档/interaction/app/shell.md`
- `docs/gold-band/产品设计文档/interaction/app/conversational-runtime.md`
- `docs/gold-band/开发计划/新UI/会话式主页实施计划.md`
- `docs/gold-band/开发计划/新UI/会话优化.md`
- `docs/gold-band/开发计划/acp接入/acp功能模块todo列表.md`

每个 Phase 完成后补充实现状态、接口和回归测试，不在实现代码中维护第二套设计说明。
