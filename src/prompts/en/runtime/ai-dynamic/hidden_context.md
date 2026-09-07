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
- Internal node (relative to Dynamic root): {{ node_dir }}
- Current attempt (relative to the internal node): {{ attempt_dir }}
- Attachments (relative to the current attempt): {{ attachments_dir }}
- Workspace ID: {{ workspace_id }}
- Workspace path: {{ workspace_path }}
- Workspace capability:
{{ workspace_capability }}

{% if has_new_round_trigger %}
## `$new-round` trigger feedback
{{ new_round_trigger }}
- This is the failed node output that opened the current new Round. Understand its failure reason and unfinished work before planning the internal tasks for this Round; do not simply repeat the original requirement unchanged.
- The artifact preview may be truncated. Read the explicitly listed artifact or attachments when complete details are needed.
{% endif %}

{% if has_coordination_snapshot %}
## Runtime coordination snapshot
- Read-only snapshot (relative to Dynamic root): {{ coordination_snapshot_path }}
- Runtime derives this file from the canonical dynamic graph and is its only writer. Do not modify it.
- Read the latest snapshot before starting or continuing this task: use each `workstreams[]` goal, TODO status, parent relationship, and steps to understand other subtasks, then use `groups[]` nesting and phase to avoid duplicate or conflicting work.
- Read the same path again before outputting `next.type="single"` or `next.type="fanout"`, and plan successors from the latest state.
{% endif %}

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
- Only attachment paths are listed; attachment contents are not read or inlined. Form a regular entry's complete path by joining `Dynamic root` with its successive path-tree levels; a top-level `absolutePath=` entry is already complete and must be used as-is.
{% if has_predecessor_attachments %}
### Predecessor chain (task handoff chain that created the current node; up to {{ source_predecessor_limit }} nodes)
{{ predecessor_attachments }}
{% if has_predecessor_attachment_overflow %}
- The attachment listings for the source nodes below are truncated or incomplete. At most {{ attachments_per_source_limit }} files or empty directories are inspected per node; non-empty directories are traversed and do not consume a slot themselves. Only found files are listed above; inspect the complete attachments directories as needed:
{{ predecessor_attachment_overflow_directories }}
{% endif %}
{% endif %}
{% if has_dependency_attachments %}
### Explicit dependencies (input nodes explicitly named through dependsOn by the current node)
{{ dependency_attachments }}
{% if has_dependency_attachment_overflow %}
- The attachment listings for the source nodes below are truncated or incomplete. At most {{ attachments_per_source_limit }} files or empty directories are inspected per node; non-empty directories are traversed and do not consume a slot themselves. Only found files are listed above; inspect the complete attachments directories as needed:
{{ dependency_attachment_overflow_directories }}
{% endif %}
{% endif %}
{% if has_group_evidence_attachments %}
### Group evidence (current merge / acceptance inputs or the latest merge and acceptance from related groups)
{{ group_evidence_attachments }}
{% if has_group_evidence_attachment_overflow %}
- The attachment listings for the source nodes below are truncated or incomplete. At most {{ attachments_per_source_limit }} files or empty directories are inspected per node; non-empty directories are traversed and do not consume a slot themselves. Only found files are listed above; inspect the complete attachments directories as needed:
{{ group_evidence_attachment_overflow_directories }}
{% endif %}
{% endif %}
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
{% if agent_strategy_mode == "dynamic" %}- Agent routing guidance:
{{ agent_routing_prompt }}
- Merge / acceptance model policy:
{{ acceptance_model_policy }}
{% endif %}- Available agents and configured runtime options:
{{ available_providers }}
- Available profiles:
{{ available_profiles }}
{% endif %}
