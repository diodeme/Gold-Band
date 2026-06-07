# 运行模式管理

## 信息架构

运行模式管理是会话式 UI 新增的独立设置页面，管理新会话创建的默认配置。

## 页面定位

- 仅管理 AUTO 设置和工作流模板管理
- 不吞并 Agent 管理和上下文管理（各自独立菜单）

## AUTO 设置

AUTO 模式本质上是一个只有 AI-DYNAMIC 节点的工作流。

### 配置项
- **Agent**：从 Agent 管理枚举已配置的 agent
- **模型**：从 agent 支持的模型列表选择
- **权限模式**：设置 AI-DYNAMIC 内部节点的默认权限模式
- **全局 Goal**：统一目标，运行时追加到每个内部节点

### 行为
- 每个 workspace 记忆上次选择的运行模式
- AUTO 模式下创建 task 前生成标准 WorkflowDsl
- 生成的 workflow 走现有 validation、snapshot、runtime

## 工作流模板管理

复用现有工作流模板管理能力，从 TaskListPage 创建抽屉中抽取。

### 功能
- 查看已保存的工作流模板列表
- 新建、编辑、删除模板
- 最后使用的模板记忆（workspace 级）

## 校验规则

创建新会话时校验：
- workspace 已选择
- AUTO 模式：agent 已选择
- WORKFLOW 模式：workflow 模板有效
- 校验失败时告知缺失项并提供恢复路径

## 与 Agent/Context 管理的边界

- Agent 管理：独立页面，管理 agent 的配置和诊断
- 上下文管理：独立页面，管理角色（profile）
- 运行模式管理：仅管理 AUTO 设置 + 工作流模板，通过下拉引用上述两项
