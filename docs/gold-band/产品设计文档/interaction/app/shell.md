# 桌面客户端应用壳与一级导航

## 1. 一句话定义
应用壳定义 Gold Band 桌面端的全局框架：左侧一级功能区固定，右侧显示当前一级功能的页面内容。

---

## 2. 页面结构

```text
┌──────────────────────────────────────────────────────────────┐
│ 共享顶栏：折叠按钮 / 品牌 icon+标题 / 可拖拽空白区 / 窗口控制               │
├───────────────┬──────────────────────────────────────────────┤
│ Logo          │ 当前一级功能区                                │
│               │                                              │
│ 一级菜单       │ 页面标题 / 面包屑 / 页面操作                    │
│ - 任务编排     │                                              │
│ - 上下文管理   │ 页面主体                                      │
│               │                                              │
│ Settings      │                                              │
└───────────────┴──────────────────────────────────────────────┘
```

---

## 3. 左侧一级功能区

### 3.1 Logo 区
位于左上角。

内容：
- Gold Band logo
- 可选：当前 workspace 名称

行为：
- 点击 logo 返回当前 workspace 的默认入口。
- 当前 MVP 中默认返回“任务编排 / 任务列表”，并重置任务编排内部的深层页面状态。

### 3.2 Workspace 选择与记忆
左侧 Logo 下方显示当前 workspace 路径，并作为”切换工作空间”入口。

规则：
- 桌面端启动时优先恢复用户上次选择的 workspace。
- 若无用户记忆，则从当前进程目录向上查找包含 `.gold-band/` 的项目根目录，避免 Tauri dev 从 `src-tauri/` 启动时误读子目录。
- 用户可通过原生目录选择器打开新的 workspace；选择后立即刷新任务编排页面栈。
- 桌面端原生目录选择器在主线程必须使用非阻塞调用；禁止在 workspace 选择链路使用 blocking dialog API，避免 macOS 上触发 event loop 卡死。
- 最近使用 workspace 写入用户级本地偏好，不属于 task / run / round canonical state。
- **新旧 UI 多工作空间职责分离**：旧 UI（工作台模式）仅维护单一全局 workspace（`DesktopContext.repo_root`），所有 task/run 操作均在该 workspace 下执行；新 UI（会话模式）维护独立的多工作空间列表（`conversation_workspaces` + `last_conversation_workspace`），通过 `projectId` 在创建/查看/操作会话时解析到对应 workspace 路径，不依赖旧 UI 的全局 workspace。
- **工作台观察期边界**：2026-07-22 起产品内隐藏 Workbench / Conversation 形态切换入口，桌面根路径默认进入 `/chat` 会话主页；旧工作台页面、路由与单 workspace 状态暂时保留，只允许通过显式 `/tasks`、`/agents`、`/contexts`、`/settings` 等 deep link 访问，不再读取历史 UI 模式偏好覆盖默认入口。
- **持久化边界**：`recent_desktop_workspaces` 仅由旧 UI 管理（`choose_workspace` / `select_recent_workspace` / `remove_recent_workspace`）；`conversation_workspaces` 和 `last_conversation_workspace` 仅由新 UI 管理（`add_conversation_workspace` / 成功创建/重跑后的 `save_last_conversation_workspace` / `remove_conversation_workspace`）。新 UI 添加、查看或草稿选择 workspace 不污染旧 UI 最近列表。
- **废弃字段边界**：`SettingsConfig.desktop_workspace` 已标记废弃，本阶段仅为旧 Workbench 的单 workspace 启动与最近列表兼容而保留，不删除、不新增消费方。会话 UI 的 workspace canonical state 只允许来自 `conversation_workspaces` 与 `last_conversation_workspace`；待旧 Workbench 删除时再一并移除该字段。
- **最近列表管理**：旧 UI workspace 选择页的最近列表每行提供打开与移除操作；移除只删除用户级 `recent_desktop_workspaces` 记录，不切换当前 workspace，不删除磁盘目录，也不影响新 UI 的 `conversation_workspaces`。当前正在使用的 workspace 不允许从最近列表移除；有效最近列表只剩一个 workspace 时也禁用移除，避免把工作台置入无当前 workspace 的状态。

### 3.3 一级菜单
当前菜单：

```text
任务编排
Agent 管理
上下文管理（占位）
模型管理（占位）
```

规则：
- 一级菜单只控制右侧功能区的根模块。
- 点击“任务编排”应回到任务列表根页面，而不是保留任务工作流或 Round 详情等深层页面。
- 当前实现任务编排、Agent 管理、上下文管理和设置。
- 上下文管理当前提供角色管理（Profile Management），用于维护工作流节点引用的 profile；列表拆成“自定义角色 / 内置角色”双 tab，自定义角色维护独立分页与过滤，内置角色单独浏览。内置角色与自定义角色卡片使用同一套紧凑密度基线，桌面常见宽度下优先维持一行 3 张卡片，再按宽度退化。浏览器预览 mock 与桌面端 Tauri 数据通路必须分层隔离，不能在正式路径复用 mock 状态；该分层由前端 Vitest 回归测试持续覆盖 runtime 选择、facade 透传和 desktop/browser 双实现语义。
- 上下文管理的 MCP 管理页按“自定义 MCP / 内置 MCP”分段展示。自定义 MCP 允许用户新增、编辑、删除和启停；内置 MCP 由渠道配置注入，只允许用户查看、诊断、查看工具列表和启停，不能编辑或删除。内置 MCP 仅在对应渠道声明 `builtinMcpServers` 时注入，首次注入使用渠道配置的 `enabled` 默认值；后续启动同步只刷新名称、连接方式和帮助信息，必须保留用户在本机选择的启停状态。MCP 工具列表通过后端 `tools/list` 获取，stdio transport 必须在同一个子进程会话中先完成 `initialize`，再等待 `tools/list` 的 JSON-RPC 响应，不能把 initialize 响应误当作工具列表结果。Gold Band 设置页和存储层继续使用内部 transport + map 结构；发送 ACP `session/new|load` 时必须转换为 ACP `mcpServers` wire format：stdio 不带 `type`，HTTP/SSE 使用 `type: "http"|"sse"`，`env` 与 `headers` 均为 `{ name, value }` 数组，不能透传内部 `id`、`transport` 或对象 map。
- 模型管理保持占位，可显示 disabled 或 coming soon 状态。
- 不在一级菜单中放 run、round、node 等任务内部对象。

### 3.4 Settings 入口
位于左下角。

行为：
- 点击进入设置页，或打开设置浮层。
- 当前包含通用、外观、高级三类设置：通用承载语言，外观承载主题与字体，高级承载更新地址覆盖和手动检查更新。
- 当后台发现当前可更新版本且用户尚未读到该版本时，Settings 入口右侧显示红点；用户进入设置页后，只清除这一层红点，不影响高级页和更新分组内更深层的提醒。

---

## 4. 右侧功能区
右侧功能区完全由当前一级功能控制。

当选中“任务编排”时，右侧页面栈为：

```text
任务列表
任务详情
工作流列表
run 详情
round 详情
```

右侧功能区顶部通常包含：
- 页面标题
- 面包屑
- 页面级操作按钮
- 当前状态摘要

---

## 5. 面包屑规则
面包屑只出现在右侧功能区，不属于左侧一级菜单。

示例：

```text
任务列表 > 任务01 > 工作流 > run01 > round01
```

行为：
- 点击“任务列表”返回任务列表页。
- “任务01”仅作为当前任务上下文标签，不可点击，不显示 hover 底线。
- 点击“工作流”返回任务工作流页；当“工作流”为当前页时只表示当前位置。
- 当前页最后一项不可点击或仅表示当前位置。
- 可点击上级项默认无下划线，鼠标悬浮或键盘 focus 时使用文字提亮与 primary 底边线作为可选中反馈；当前页使用更短的金色渐变底线表示当前位置；不可点击项不显示底线，只显示文本状态。

---

## 6. 桌面端交互原则

### 6.1 保持稳定区域
- 左侧一级功能区不随页面下钻变化。
- 右侧功能区随页面层级变化。
- 用户始终知道自己处于哪个一级模块。

### 6.2 避免终端心智
应用壳不提供：
- command bar
- slash command
- terminal input
- chat input

全局操作应放入：
- 菜单
- 按钮
- 右键菜单
- 设置页
- 快捷键

### 6.3 适配桌面窗口
功能区应支持：
- 面板宽度调整
- 列表滚动
- 详情页滚动
- 大屏展示更多列
- 小窗口下保留左侧一级导航和当前页面核心内容
- 应用整窗统一使用共享顶栏，不保留额外原生 header；macOS 仅保留系统左上角 traffic lights，Windows/Linux 使用顶栏右侧自定义窗口按钮
- 顶栏颜色跟随当前主题切换，浅色与深色主题都维持同一套结构
- 顶栏左侧只保留一个侧边栏折叠按钮；macOS 在按钮前为原生 traffic lights 预留安全间距；观察期内不展示 Workbench / Conversation 形态切换控件
- 共享顶栏除按钮、输入等交互控件外都属于窗口拖拽命中区；Windows/Linux 使用 Tauri `startDragging` 与 WebView app-region 共同保证拖拽稳定性，交互控件必须显式退出拖拽区
- 侧边栏折叠/展开使用平滑宽度过渡，不做瞬时消失；内容透明度可略早于宽度收起，以减少视觉突兀
- 顶栏与侧边栏默认共用同一 surface 底色，并去掉强横向分割线；右侧主区使用更弱的 top/left 边界与左上圆角衔接，主区圆角后方露出的底色继续复用 sidebar surface，而不是把侧边栏自身裁成圆角，避免角后方出现异色小方块
- 桌面外轮廓、阴影与圆角统一由宿主窗口 compositor 管理；应用根壳不得再绘制整窗 border、圆角或高层级伪元素。CSS 圆角与 Win11 DWM 圆角存在独立半径和抗锯齿路径，叠加会产生突出的双层轮廓；Windows 无边框窗口保留 native shadow，以获得系统提供的 1px frame 与圆角。
- Windows/Linux 顶栏右侧承载窗口最小化、最大化/还原、关闭操作；macOS 使用系统原生左上角 traffic lights，顶栏不重复渲染自定义窗口按钮
- Windows/Linux 自定义窗口控制组使用 `w-max + flex-none` 保持 intrinsic width，组内最小化、最大化/还原、关闭三个按钮也必须分别使用 `flex-none`，禁止嵌套 Flex 在窄窗口下压缩按钮宽度；关闭按钮按 Windows 原生标题栏习惯贴紧右侧窗口边缘，不额外设置右侧 gutter。
- 桌面窗口最小尺寸只由 Tauri Window 配置管理；`html/body/#root` 不得重复设置固定 `min-width/min-height`。WebView 必须始终服从真实 viewport 尺寸并继续触发响应式布局，禁止在达到 CSS 最小宽度后保持旧布局、由原生窗口直接裁切右侧内容。
- 桌面壳内的二级布局必须基于实际内容容器宽度决定分栏，而不是直接复用整窗 `md/lg/xl` breakpoint。侧边栏、section 标题列、抽屉和详情 inspector 都会减少真实可用宽度；嵌套区域优先使用 Tailwind container query，固定画布/表格则必须提供明确的换行、堆叠或横向滚动降级策略。
- Windows 无边框窗口使用 WebView2 composition 路径承载连续边缘 resize：Tauri Window 在 Windows 平台启用透明控制器，但 `html/body/#root` 与应用壳始终绘制不透明主题 surface，不向用户呈现实际透明效果。宿主 Window 背景随主题同步，WebView 层保持 composition 模式，禁止为了遮盖黑带重新设为不透明控制器。
- 主窗口初始隐藏；bootstrap、主题和宿主背景准备完成后由前端显式显示，避免 transparent composition 窗口在首帧 CSS 尚未就绪时闪现。Windows 自定义无边框模式保留 native shadow，由 DWM 在 Win11 提供系统圆角；macOS 恢复 native decorations、shadow 与 traffic lights。
- 上下文管理的角色卡片网格按列表容器实际宽度决定列数，而不是只依赖整窗 viewport：窄容器单列、中等容器双列、足够宽时三列。卡片底部操作区允许整组换行，系统显示缩放、字体增大或翻译文案变长时不得越过卡片边界或覆盖相邻卡片。

### 6.4 运行态生命周期
- Round 详情页的“继续运行”只在当前 run / round / node 处于可恢复暂停态时出现；成功、失败或 killed 的终局 round 不展示该入口。
- 顶部“继续运行”是 workflow runtime 控制动作，不等同于 ACP 会话抽屉中的自由输入 composer；它会向当前 ACP session 自动发送本地化 `继续 / Continue`。
- ACP 会话抽屉属于右侧功能区的可关闭详情层；关闭抽屉不取消正在发送的 prompt，同一 attempt 重新打开时需要恢复发送中的乐观用户消息和 composer 锁定状态。
- 桌面窗口关闭时，应用壳负责 best-effort 停止当前 workspace 内仍为 `running` 的 run，确保 provider 进程和 canonical run lifecycle 一致。

---

## 7. Tauri 2.x MVP 对应实现

MVP 中应用壳由 `web/src/components/Shell.tsx` 实现：
- 左侧固定展示 Gold Band、workspace 路径/切换入口、任务编排、Agent 管理、上下文管理、模型管理占位、设置。
- 右侧由 React 状态维护当前一级模块内容；任务编排继续使用递进式页面栈，Agent 管理和上下文管理为独立管理页。
- 工作空间选择页由 `web/src/pages/WorkspaceSelectPage.tsx` 实现，展示原生选择按钮和最近 workspace 列表；主视觉入口使用与侧边栏一致的 Gold Band logo，不使用临时菱形占位图标。该页面只作为工作台模式覆盖层渲染，不进入会话模式页面栈。
- Tauri commands `choose_workspace` / `select_recent_workspace` 负责切换 workspace，并将最近列表写入用户级配置；`remove_recent_workspace` 只移除最近列表项并返回刷新后的 bootstrap。
- `choose_workspace` 与会话侧 `add_conversation_workspace` 必须统一复用非阻塞目录选择封装，避免同类原生弹窗行为分叉。
- 桌面端必须为 `choose_workspace` / `select_recent_workspace` 记录结构化系统日志，至少覆盖“打开目录选择器”“用户取消”“目录返回”“切换完成”四个阶段，便于排查 macOS 原生目录选择器卡死或切换后状态未刷新问题。
- Tauri window 默认尺寸为 1280x800，最小尺寸为 1040x680；这是桌面窗口最小尺寸的唯一事实源，Web 层不得镜像同一固定值。
- 应用壳不提供命令输入、slash command、terminal input 或 chat input。
- 2026-05-03 起应用壳使用 Tailwind CSS v4 + shadcn/ui Button、Tooltip、Separator 等现成组件重构；侧边栏 IA、workspace 切换入口和右侧页面栈行为不变。
- 2026-06-08 起新旧 UI 共用 `web/src/components/AppTitleBar.tsx` 共享顶栏；Tauri 基础配置关闭 WebView file-drop，避免与 composer 附件拖拽上传争抢文件 drop。
- 2026-06-19 起桌面 bootstrap 暴露 `platform` 作为前端唯一平台事实源：macOS 启用原生 traffic lights + overlay 标题栏并隐藏系统标题文本；Windows/Linux 继续关闭整窗 decorations，由共享顶栏右侧自定义窗口控制接管最小化、最大化/还原和关闭。
- 2026-06-29 起共享顶栏拖拽命中升级为“整条顶栏默认可拖，交互控件显式 no-drag”；早期根壳 overlay 外轮廓方案已由 2026-07-23 的 native compositor 方案替换。
- 2026-07-22 起共享顶栏隐藏 Workbench / Conversation toggle，根路径统一进入会话主页；旧工作台仅保留显式 deep link，供观察期继续验证而不暴露产品入口。
- 2026-07-23 起共享顶栏取消 Windows/Linux 窗口控制组的末尾 gutter，并固定控制组及每个按钮的 intrinsic width；删除 Web 层 `1040x680` 最小尺寸镜像，让真实 viewport 持续驱动响应式布局。角色管理卡片网格改为容器查询升列，卡片操作区支持换行，以覆盖 Win10 1080p 显示缩放和最小窗口场景。
- 2026-07-23 Windows 边缘缩放修正：采用 Tauri/WebView2 transparent composition workaround，并复用 AionUi 的“窗口初始隐藏、首帧完成后显示”启动策略；保留 native shadow 以获得 Win11 DWM 原生圆角和边框，删除根壳的整窗 border、CSS 圆角与高层级伪元素。

---

## 8. 一句话总结

> 应用壳只解决“我在哪个一级功能里”，任务内部的递进浏览全部发生在右侧功能区。
