# AI-DYNAMIC runtime context for this invocation

## Current dynamic node
- Parent node: {{ outer_node_id }}
- Parent attempt: {{ outer_attempt_id }}
- Dynamic run: {{ dynamic_run_id }}
- Internal node: {{ node_id }}
- Title: {{ title }}
- Kind: {{ kind }}
- Group: {{ group_id }}
- Chain: {{ chain_id }}
- Depth: {{ depth }}

## Runtime location
- Dynamic root: {{ dynamic_root }}
- Internal node dir: {{ node_dir }}
- Internal attempt dir: {{ attempt_dir }}
- Internal attachments dir: {{ attachments_dir }}
- Workspace mode: {{ workspace_mode }}
- Workspace path: {{ workspace_path }}
- Workspace capability:
{{ workspace_capability }}

{% if has_direct_predecessors %}
## Direct predecessors
{{ direct_predecessors }}
{% endif %}

{% if has_active_group %}
## Active group
{{ active_group }}
{% endif %}

{% if has_inherited_groups %}
## Inherited group context
{{ inherited_groups }}
{% endif %}

{% if has_siblings %}
## Parallel siblings
{{ siblings }}
{% endif %}

{% if has_available_attachments %}
## Available attachments
{{ available_attachments }}
{% endif %}

{% if has_output_contract %}
## Session reuse
- Session mode: {{ session_mode }}
- continueFromNodeId: {{ continue_from_node_id }}
- Note: `continue` only reuses the source node's ACP session context; the current task is the `# Task` in this user prompt.
- Resumable session nodes in the current chain:
{{ resumable_sessions }}

## Runtime limits
- Allowed workflow snapshots:
{{ allowed_workflow_snapshots }}
- Remaining budget:
{{ remaining_budget }}

## Agent and profile options
- Dynamic node agent strategy: {{ agent_strategy_mode }}
- Bootstrap agent: {{ bootstrap_provider }}
{% if agent_strategy_mode == "dynamic" %}- Agent and model routing guidance:
{{ agent_routing_prompt }}
- Merge / acceptance model policy:
{{ acceptance_model_policy }}
{% endif %}- Available agents and models:
{{ available_providers }}
- Available profiles:
{{ available_profiles }}
{% endif %}
