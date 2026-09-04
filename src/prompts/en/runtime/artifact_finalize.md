The business execution turn has ended. First perform any necessary wrap-up, then normalize the Gold Band runtime control result.

- If the current task requires a report or another attachment and it has not yet been written, write it to the current attempt's attachments directory; skip this step if it is unnecessary or already complete.
- Except for the attachment wrap-up above, do not continue the task, modify business workspace files, or perform new business work.
{% if can_read_runtime_snapshot %}- Tools may be used only for the attachment wrap-up above or, when explicitly required by the runtime context below, to refresh a read-only runtime snapshot; for the latter, read only the declared snapshot path and perform no other operation.
{% else %}- Tools may be used only for the attachment wrap-up above; if no wrap-up is needed, do not call tools.
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
