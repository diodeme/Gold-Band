# 本次 AI-DYNAMIC 运行上下文

## 当前动态节点
- 父节点：{{ outer_node_id }}
- 父 attempt：{{ outer_attempt_id }}
- Dynamic run：{{ dynamic_run_id }}
- 内部节点：{{ node_id }}
- 标题：{{ title }}
- 节点类型：{{ kind }}
- 所属 group：{{ group_id }}
- 所属 chain：{{ chain_id }}
- 当前深度：{{ depth }}

## 运行位置
- Dynamic 根目录：{{ dynamic_root }}
- 内部节点目录：{{ node_dir }}
- 内部 attempt 目录：{{ attempt_dir }}
- 内部 attachments 目录：{{ attachments_dir }}
- Workspace ID：{{ workspace_id }}
- Workspace 路径：{{ workspace_path }}
- Workspace 能力：
{{ workspace_capability }}

{% if has_coordination_snapshot %}
## Runtime 协调快照
- 只读快照：{{ coordination_snapshot_path }}
- 该文件由 Runtime 从 canonical dynamic graph 生成并独占写入；不要修改它。
- 开始或继续当前任务前读取最新快照：先按 `workstreams[]` 的目标、TODO 状态、父子关系与 steps 理解其他子任务，再结合 `groups[]` 的嵌套关系和 phase，避免重复或冲突。
- 准备输出 `next.type="single"` 或 `next.type="fanout"` 前再次读取同一路径，以最新状态规划后继任务。
{% endif %}

{% if has_direct_predecessors %}
## 直接前序节点
{{ direct_predecessors }}
{% endif %}

{% if has_active_group %}
## 当前 group
{{ active_group }}
{% endif %}

{% if has_inherited_groups %}
## 继承的 group 上下文
{{ inherited_groups }}
{% endif %}

{% if has_siblings %}
## 并行兄弟节点
{{ siblings }}
{% endif %}

{% if has_available_attachments %}
## 可用附件
{{ available_attachments }}
{% endif %}

{% if has_output_contract %}
## 会话复用
- Session mode：{{ session_mode }}
- continueFromNodeId：{{ continue_from_node_id }}
- 说明：`continue` 只表示复用来源节点的 ACP session 上下文；当前任务以本次 user prompt 的 `# 任务` 为准。
- 当前链路可复用会话节点：
{{ resumable_sessions }}

## 运行预算
- Allowed workflow snapshots：
{{ allowed_workflow_snapshots }}
- 剩余预算：
{{ remaining_budget }}

## Agent 与 profile 选项
- 动态节点 agent 策略：{{ agent_strategy_mode }}
- 初始分发节点 agent：{{ bootstrap_provider }}
{% if agent_strategy_mode == "dynamic" %}- Agent 决策指南：
{{ agent_routing_prompt }}
- merge / acceptance 模型策略：
{{ acceptance_model_policy }}
{% endif %}- 可用 agent 及预配运行参数：
{{ available_providers }}
- 可用 profiles：
{{ available_profiles }}
{% endif %}
