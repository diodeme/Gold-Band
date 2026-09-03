The business execution turn has ended. Now only normalize the Gold Band runtime control result.

- Do not continue the task, modify files, or perform new business work.
{% if can_read_runtime_snapshot %}- Only when the runtime context below explicitly requires refreshing a read-only runtime snapshot may you call a tool; read only the declared snapshot path and do not call any other tool.
{% else %}- Do not call tools.
{% endif %}
- Produce the canonical artifact only from work already completed in this conversation.
- Do not output explanations, Markdown, code fences, or any extra content.
{% if finalize_context %}
The following runtime context is only for this control-result normalization:
{{ finalize_context }}
{% endif %}

Output artifact: {{ artifact }}
Output kind: {{ kind }}

Output only content that follows this protocol:
{{ schema }}{% if success_condition %}

runtime will subsequently evaluate the node result using this condition:
{{ success_condition }}{% endif %}
