The last `dynamic-node-completion` proposal was invalid and rejected by runtime.

You must repair the final `dynamic-node-completion` output so it satisfies the runtime constraints below.
Repair only protocol validation errors; do not re-execute the task. Successor work must still satisfy the scope contract; remove or narrow out-of-scope items.
Do not output explanations, Markdown, code fences, or any extra text. Output only the repaired `dynamic-node-completion` content.

{% if has_coordination_snapshot %}Latest coordination snapshot:
- Read-only snapshot: {{ coordination_snapshot_path }}
- Read the latest coordination snapshot before repairing and outputting `next.type="single"` or `next.type="fanout"`; read only and do not modify this file.
{% endif %}

Validation errors:
{{ validation_errors }}

Current valid value reference:
{{ repair_reference }}

Current remaining budget:
{{ remaining_budget }}
