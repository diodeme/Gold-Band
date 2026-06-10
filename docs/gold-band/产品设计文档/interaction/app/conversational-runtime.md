# 会话式运行时

## 信息架构

会话运行时窗口是用户与 agent 交互的核心区域。左侧选中最小单位是 run，右侧主区域永远展示当前选中 session 的具体对话。

## 顶部信息栏

- 标题显示：可 inline edit，修改后同步到 task 和所有 run
- 标题修改后不再被自动覆盖

## 顶部操作栏

### 重跑按钮
- 常显，icon 为新建
- 当前 run 运行中：弹窗二次确认，确认后停止当前 run → 创建新 run
- 当前 run 已结束：直接创建新 run
- run 历史始终保留

### 编辑工作流
- WORKFLOW 模式下显示查看按钮（Eye 图标）和编辑按钮（Workflow 图标）
- 查看工作流：打开 Sheet，复用旧 UI 的运行态工作图组件与数据链路，展示当前选中 session 所在 round 的实际路径图
- 查看工作流中的节点状态、暂停/成功图标、产物数、附件数、agent 标识等信息应与旧 UI 保持一致
- 编辑工作流：打开 Sheet，内嵌 WorkflowEditor 完整编辑器
- 修改只影响未来 run，不影响当前 run snapshot

## Session Switcher

- 位于会话窗口顶部信息展示区
- 显示路径如 `round-001/dev/attempt-002`
- 点击展开 round → node → attempt 层级树
- 用户可切换具体 session

### 默认 session 选择
- 用户已有选中 session 且仍有效时保持
- 多个 session 默认最近 session
- run 结束时显示到达 end 状态的 session

### 自动切换规则
- 上一个 session 完成 + 消息窗口在底部 → 自动切换并折叠历史
- 用户不在底部（正在看历史）→ 不自动切换、不折叠
- 只有一个 session 运行中 → 自动展开该 session
- 多个 session 运行中 → 显示折叠行（session 名 + 实时状态），用户点击进入

## Composer 附件

继续对话时可上传附件作为本轮输入内容：

- **入口**：纸夹按钮、拖拽、粘贴（统一走 same-session 附件模型）；桌面端文件进入 WebView 后即声明可拖拽，拖入 composer 时应稳定显示可投放状态
- **预览**：图片文件在 composer 内显示缩略图，点击可打开沉浸式大图预览；预览使用单层深色遮罩按合适尺寸展示原图，不支持缩放或拖拽，点击空白遮罩关闭
- **传输**：输入附件作为 ACP content block 发送给 agent，不混入 agent 输出产物目录

## Composer 状态

### 互斥状态
1. **正常输入**：用户可自由输入消息（含附件）
2. **继续按钮**：当前 session 暂停可继续时，显示继续按钮
3. **工作流无效修复按钮**：工作流无效时，显示修改按钮
4. **运行错误提示/操作**：显示错误原因和操作入口

### 继续输入
- `continueRun` 继续同一个 session
- 不进入工作流 runtime
- 复用现有实现

### 停止
- 停止并重跑在顶部操作区
- composer 内也有 stop 按钮（ACP 会话停止）

## 会话信息栏（ACPSessionHeader）

- 单行布局：模型名 + 权限模式 Badge + sessionId + 操作按钮
- 有产物/附件时：系统提示和原始帧按钮左侧显示"查看产物"按钮（Package 图标）
- 产物来源固定为当前选中 session（含 AI-DYNAMIC 内部节点）的 artifacts / attachments，不使用 run 级聚合占位数据
- 点击查看产物：弹窗列表展示当前选中 session 的所有 artifacts 和 attachments，点击具体项加载并显示内容
- 产物弹窗遮罩使用轻量弱化遮罩（低透明深色 + blur），主体面板保持半透明而不过度强调，不做厚重黑色卡片
- sessionId 与模型名、权限模式同行，不再单独占行

## 产物/附件信息区

- 位于 composer 下方
- 三区展示：输入附件 / 产物 / 附件（输出）
- 输入附件来源于 task 级 authoring/attachments/，创建会话时设定，重跑自动复用
- 输入附件使用 Upload 图标 + 蓝色标记，与输出产物/附件区分
- 点击查看详情，图片类附件以 base64 预览展示

## 附件生命周期

- 新会话附件绑定 task，作为初始输入的一部分，持久化到 authoring/attachments/
- 重跑复用 task-level 附件（同一 task 的 authoring/attachments/ 在多次 run 间共享）
- 继续对话新附件进入当前 ACP session，发送后以文本形式告知 agent 文件名
- 输入附件展示为独立层级，不与 agent 运行产物和输出附件混合
