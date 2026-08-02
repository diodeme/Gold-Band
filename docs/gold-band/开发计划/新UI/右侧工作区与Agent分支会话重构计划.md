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
- 右侧区域是跨页面可复用的辅助资源工作区，不属于 `ConversationRunPage` 的局部抽屉。
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
- 关闭最后一个 Tab 后收起右侧工作区。
- Tab 过多时允许横向滚动并提供溢出列表，不压缩到不可读宽度。
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
  width: number;
  autoCollapsed: boolean;
}
```

Tab 描述只保存资源定位，不直接保存 timeline、文件内容等大对象。资源缓存、实时状态和 DOM 生命周期独立管理。

## 6. 面板宽度与窗口响应式

### 6.1 组件选择

- 优先引入 shadcn `Resizable` copy-in 组件，底层使用成熟的 `react-resizable-panels`。
- 左侧导航、中间主工作区、右侧工作区进入统一水平 Panel Group。
- 不继续扩展当前应用壳中手写的 `mousemove/mouseup` 拖拽算法。
- shadcn Sheet 只用于紧凑宽度下的右侧资源覆盖模式，不作为常驻 Dock。
- 拖拽命中区域保持足够宽，但可见边界只使用低对比 1px 分隔，不显示粗色带。

### 6.2 页面布局配置

原生窗口最小宽度保持全局稳定；当前页面通过集中式布局配置声明中间区域最小宽度，用于决定面板何时收起。不得在页面组件中散落硬编码判断。

```ts
interface WorkspaceLayoutProfile {
  centerMinWidth: number;
}

const WORKSPACE_LAYOUT_PROFILES = {
  conversation: { centerMinWidth: 420 },
  contextCards: { centerMinWidth: 520 },
  workflowCanvas: { centerMinWidth: 640 },
  settings: { centerMinWidth: 480 },
} satisfies Record<string, WorkspaceLayoutProfile>;
```

以上数值是设计初值，实施阶段必须通过真实页面测量和最小窗口人工验证校准。

### 6.3 横向收缩顺序

窗口横向缩小时：

1. 右侧工作区先收缩到自己的最小宽度。
2. 中间区域即将低于当前页面 `centerMinWidth` 时，自动收起左侧导航。
3. 继续缩小且中间区域再次接近最小宽度时，自动收起右侧工作区。
4. 剩余宽度全部交给中间区域。
5. 达到 Tauri 原生窗口全局最小宽度后停止缩小。

窗口变宽时按相反顺序恢复：

1. 先恢复右侧工作区。
2. 再恢复左侧导航。

临界判断必须提供迟滞区间，避免窗口拖动时左右区域反复闪烁。手动折叠状态与自动折叠状态分别建模；自动恢复不得覆盖用户主动关闭右侧工作区的意图。

### 6.4 手动拖拽规则

- 用户拖动右侧分隔线时，左侧导航不自动关闭。
- 右侧宽度在统一最小值、最大值之间变化。
- 中间区域达到当前页面最小宽度后，继续拖动不再扩大右侧区域。
- 右侧宽度保存到会话 UI preference，不能只保存到临时组件 state。
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
6. 分隔线拖动和整窗 resize 只更新布局状态，不重建 timeline projection。
7. ResizeObserver 和宽度策略按 animation frame 批量处理，避免布局读写交错。
8. Agent summary 更新与 Agent transcript streaming 分层，主会话不消费子分支全部事件。
9. 分页仍基于 cursor 和有限窗口，不扫描前端完整历史计算 total。
10. 禁止为了多个已打开 Tab 将多个完整 ConversationViewport 长期隐藏挂载。

## 15. 实施阶段

### Phase 1：应用壳与右侧工作区基础

状态：已实现。

- 把 `ConversationShell` 提升为通用三段式 `WorkspaceShell`。
- 引入 shadcn Resizable copy-in 和 `react-resizable-panels`。
- 建立布局 profile、自动折叠状态机、迟滞和宽度 preference。
- 实现通用 Tab model、激活、关闭、去重、溢出列表。
- 使用静态 Agent resource 验证 docked/compact Sheet 两种模式。

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

## 16. 测试与验收

### 16.1 应用壳

- 三栏均打开时，拖动右栏不能把中间区域压到当前页面最小宽度以下。
- 缩小时先自动收起左栏，再收起右栏；放大时先恢复右栏，再恢复左栏。
- 临界宽度附近来回拖动不闪烁。
- 自动收起不关闭 Tab，手动关闭状态不会被自动恢复覆盖。
- 紧凑宽度点击 Agent 能以 Sheet 打开，恢复宽度后能回到 Dock。
- 会话、上下文卡片、工作流画布使用各自容器宽度降级，不依赖整窗 breakpoint。

### 16.2 会话与分页

- 500 个嵌套工具事件、2 个顶层 Agent 的根会话只显示用户消息和 2 个 Agent link，且 `hasOlder=false` 时不显示历史提示。
- Agent index 中有 2 个顶层、25 个嵌套 execution 时，根 projection 只返回 2 个顶层 Agent；任一 Agent branch 只返回 `parentAgentExecutionId` 指向自身的直属孩子。
- 活动中 100 个工具和思考事件只形成一个会话语义块。
- 折叠、展开活动不改变会话 cursor 和 `hasOlder`。
- 展开活动只加载最近审计行，并可在内部“显示更早活动”。
- Agent 分支的语义分页与根分支使用同一套接口和组件。
- Agent link 只加载对应分支，不触发其他分支重新投影。

### 16.3 生命周期与交互

- Agent launch tool 已完成但分支仍在生成时，Agent 状态保持 running。
- Agent 只有 launch、尚无内容时显示 queued；产生工具或文字后进入 running。
- 根会话停止后所有活动 Agent 收敛为 interrupted。
- Agent 内权限申请使对应链接和 Tab 出现 attention；进入 Tab 后可以决策。
- 已决权限不出现在活动审计详情。
- TODO 只出现在所属分支，不平铺回根会话。
- 存在 Agent execution 时，根 timeline 中没有 relation 的 session-wide plan 不生成 Todo；明确 scoped 的 Agent plan 仍在 Agent Tab 展示；没有 Agent execution 的普通根 plan 仍展示。
- Todo 归属测试必须证明实现不读取条目自然语言进行 Agent 匹配。

### 16.4 实时和性能

- 更新 Agent B 时，Agent A 和根历史项保持对象引用。
- 非激活 Tab 不挂载 ConversationViewport DOM。
- 非激活 Agent streaming 不持续驱动完整 Tab React render。
- 切换 Tab 能恢复各自滚动位置、分页窗口和贴底状态。
- 工具大输出在活动和工具折叠时不解析。
- 原生滚动分页锚点在主会话和 Agent 会话中均稳定，无跳动和错位。

### 16.5 持久化

- 根事件只进入根 timeline，Agent 事件只进入所属 Agent timeline。
- Agent index 能恢复父子层级、状态和统计。
- 应用重启后打开 Agent link 能恢复历史并继续接收实时更新。
- 文件名和目录只使用 Gold Band 稳定 ID，不使用未经处理的 provider ID。
- storage query 使用结构化错误码，不返回后端对客文案。

## 17. 文档同步要求

实现阶段必须同步维护：

- `docs/gold-band/产品设计文档/interaction/app/shell.md`
- `docs/gold-band/产品设计文档/interaction/app/conversational-runtime.md`
- `docs/gold-band/开发计划/新UI/会话式主页实施计划.md`
- `docs/gold-band/开发计划/新UI/会话优化.md`
- `docs/gold-band/开发计划/acp接入/acp功能模块todo列表.md`

每个 Phase 完成后补充实现状态、接口和回归测试，不在实现代码中维护第二套设计说明。
