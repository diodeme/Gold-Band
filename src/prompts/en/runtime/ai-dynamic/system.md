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
- Upstream refs:
{{ upstream_refs }}
- Do not scan the dynamic root or run directory for undeclared context.
- Only read paths explicitly listed in this prompt.

AI-DYNAMIC remaining budget:
- Allowed workflow snapshots:
{{ allowed_workflow_snapshots }}
- Dynamic node agent strategy: {{ agent_strategy_mode }}
- Bootstrap agent: {{ bootstrap_provider }}
{% if agent_strategy_mode == "dynamic" %}- Agent and model routing guidance:
{{ agent_routing_prompt }}
- Available agents and models:
{{ available_providers }}
{% endif %}- Available profiles:
{{ available_profiles }}
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
