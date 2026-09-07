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
- 内部节点（相对 Dynamic 根目录）：{{ node_dir }}
- 当前 attempt（相对内部节点）：{{ attempt_dir }}
- attachments（相对当前 attempt）：{{ attachments_dir }}
- Workspace ID：{{ workspace_id }}
- Workspace 路径：{{ workspace_path }}
- Workspace 能力：
{{ workspace_capability }}

{% if has_new_round_trigger %}
## `$new-round` 触发反馈
{{ new_round_trigger }}
- 这是上一轮触发当前新 Round 的失败节点输出。先理解其中的失败原因和未完成项，再规划本轮内部任务，不要只按原始需求原样重跑。
- artifact 预览可能被截断；需要完整信息时读取上面明确列出的 artifact 或附件。
{% endif %}

{% if has_coordination_snapshot %}
## Runtime 协调快照
- 只读快照（相对 Dynamic 根目录）：{{ coordination_snapshot_path }}
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
- 以下只列附件路径，不读取或内联附件正文。普通条目的完整路径按 `Dynamic 根目录` 与路径树各层依次拼接；顶层 `absolutePath=` 条目已经是完整路径，直接使用。
{% if has_predecessor_attachments %}
### 前序链路（创建当前节点的任务接力链，最多回溯 {{ source_predecessor_limit }} 个节点）
{{ predecessor_attachments }}
{% if has_predecessor_attachment_overflow %}
- 以下来源节点的附件清单已截断或未完整读取；每个节点最多检查 {{ attachments_per_source_limit }} 个文件或空目录，含内容的目录会继续递归且不单独计数。上方只列找到的文件，其余请按需查看完整 attachments 目录：
{{ predecessor_attachment_overflow_directories }}
{% endif %}
{% endif %}
{% if has_dependency_attachments %}
### 显式依赖（当前节点通过 dependsOn 明确指定的输入节点）
{{ dependency_attachments }}
{% if has_dependency_attachment_overflow %}
- 以下来源节点的附件清单已截断或未完整读取；每个节点最多检查 {{ attachments_per_source_limit }} 个文件或空目录，含内容的目录会继续递归且不单独计数。上方只列找到的文件，其余请按需查看完整 attachments 目录：
{{ dependency_attachment_overflow_directories }}
{% endif %}
{% endif %}
{% if has_group_evidence_attachments %}
### Group 证据（当前 merge / acceptance 输入或相关 group 最近一轮合并与验收）
{{ group_evidence_attachments }}
{% if has_group_evidence_attachment_overflow %}
- 以下来源节点的附件清单已截断或未完整读取；每个节点最多检查 {{ attachments_per_source_limit }} 个文件或空目录，含内容的目录会继续递归且不单独计数。上方只列找到的文件，其余请按需查看完整 attachments 目录：
{{ group_evidence_attachment_overflow_directories }}
{% endif %}
{% endif %}
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
