The business execution turn has ended. Now only normalize the Gold Band runtime control result.

- Do not continue the task, modify files, call tools, or perform new business work.
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
