AI-DYNAMIC runtime context:
- Parent node: {{ outer_node_id }}
- Parent attempt: {{ outer_attempt_id }}
- Dynamic run: {{ dynamic_run_id }}
- Internal node: {{ node_id }}
- Kind: {{ kind }}
- Group: {{ group_id }}
- Chain: {{ chain_id }}
- Depth: {{ depth }}

AI-DYNAMIC filesystem rules:
- Dynamic root: {{ dynamic_root }}
- Internal node dir: {{ node_dir }}
- Internal attempt dir: {{ attempt_dir }}
- Internal attachments dir: {{ attachments_dir }}
- Workspace mode: {{ workspace_mode }}
- Workspace path: {{ workspace_path }}
- Workspace capability:
{{ workspace_capability }}
- Upstream refs:
{{ upstream_refs }}
- Treat Workspace path as the current workspace for all reads and writes; `worktree` mode may only modify that worktree, and `main` mode is for merging or acceptance in the main workspace.
- Fan-out branches must not modify other branch worktrees; merge nodes only merge the current group branches listed in the kind-specific context.
- Do not scan the dynamic root or run directory for undeclared context.
- Only read paths explicitly listed in this prompt.

AI-DYNAMIC agent and model strategy:
- Dynamic node agent strategy: {{ agent_strategy_mode }}
- Bootstrap agent: {{ bootstrap_provider }}
{% if agent_strategy_mode == "dynamic" %}- Agent and model routing guidance:
{{ agent_routing_prompt }}
- Merge / acceptance model policy:
{{ acceptance_model_policy }}
- Available agents and models:
{{ available_providers }}
{% endif %}- Available profiles:
{{ available_profiles }}

AI-DYNAMIC remaining budget:
- Allowed workflow snapshots:
{{ allowed_workflow_snapshots }}
- Remaining budget:
{{ remaining_budget }}

AI-DYNAMIC execution summary:
{{ graph_summary }}
- Resumable session nodes in the current chain:
{{ resumable_sessions }}
- dependsOn: {{ depends_on }}
- kind-specific context:
{{ kind_specific_context }}

AI-DYNAMIC control protocol:
- Runtime, not you, materializes proposals and transitions.
- Every internal worker must finish with the `dynamic-node-completion` artifact.
- Use `next.type="end"` when this chain has no more work, `single` for one successor, or `fanout` for parallel branches.
