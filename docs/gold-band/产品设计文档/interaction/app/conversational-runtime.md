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
- WORKFLOW 模式下显示工作流 icon
- 点击打开 workflow drawer
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

## Composer 状态

### 互斥状态
1. **正常输入**：用户可自由输入消息
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

## 产物/附件信息区

- 位于 composer 下方
- 有内容才显示
- 点击查看详情
- 显示当前 session 的产物和附件数量

## 附件生命周期

- 新会话附件绑定 task，作为初始输入的一部分
- 重跑复用 task-level 附件
- 继续对话新附件进入当前 ACP session
