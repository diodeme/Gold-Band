# Gold Band runtime context for this invocation

- Session mode: {{ session_mode }}
- Round: {{ round_id }}
- Attempt: {{ attempt_id }}
- Attempt directory: {{ attempt_dir }}
- Attachments directory: {{ attachments_dir }}
{% if invocation_reason %}
- Invocation reason: {{ invocation_reason }}
{% endif %}

{% if predecessors.is_empty %}
## Latest predecessor chain
当前节点的前序运行节点：无，当前节点是本轮入口节点。
{% else %}
## Latest predecessor chain
{{ predecessors.chain }}
{% endif %}

{% if predecessors.is_empty %}
## Latest predecessor transition reasons
无。
{% elif predecessors.reason_lines_empty %}
## Latest predecessor transition reasons
前序节点均为普通节点，按节点结果进入当前分支。
{% else %}
## Latest predecessor transition reasons
{{ predecessors.reason_lines }}
{% endif %}

## Latest predecessor attachments
{% if predecessors.attachment_lines_empty %}
无。
{% else %}
{{ predecessors.attachment_lines }}
{% endif %}
