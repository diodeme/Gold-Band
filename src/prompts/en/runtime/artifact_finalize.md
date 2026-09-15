This response turn ended, but runtime has not received this node's artifact. The protocol below is being provided or repeated; this does not mean the task is complete or require an early handoff. Decide whether you intend to finish this node.

- If you did not intend to finish this node, continue executing directly within the current task scope, workspace, and tool permissions. No status tag is required, and do not hand off early just to answer this prompt. After continuing, output the artifact below when you consider this node finished.
- If you consider this node finished, output the artifact below based on the work completed. Do not re-audit the task goals or acceptance requirements, or add business work because of this prompt.
- Before emitting the artifact, if the current task requires a report or another attachment and it has not yet been written, write it to the current attempt's attachments directory; skip this step if it is unnecessary or already complete.
{% if can_read_runtime_snapshot %}- When preparing the artifact, refresh a read-only runtime snapshot only when explicitly required by the runtime context below; read only the declared snapshot path.
{% endif %}- While continuing execution, reply and use tools normally. Only the final artifact output must omit explanations, Markdown, and code fences.
{% if finalize_context %}
The following runtime context is only for this control-result normalization:
{{ finalize_context }}
{% endif %}

Output artifact: {{ artifact }}
Output kind: {{ kind }}

Follow this protocol only when you consider this node finished and decide to emit the artifact:
{{ schema }}{% if success_condition %}

runtime will subsequently evaluate the node result using this condition:
{{ success_condition }}{% endif %}
