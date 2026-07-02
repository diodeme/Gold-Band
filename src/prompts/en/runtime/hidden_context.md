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
Previous executed nodes: none. This node is the entry node for the current round.
{% else %}
## Latest predecessor chain
{{ predecessors.chain }}
{% endif %}

{% if predecessors.is_empty %}
## Latest predecessor transition reasons
None.
{% elif predecessors.reason_lines_empty %}
## Latest predecessor transition reasons
All previous nodes were ordinary transitions based on node outcome.
{% else %}
## Latest predecessor transition reasons
{{ predecessors.reason_lines }}
{% endif %}
