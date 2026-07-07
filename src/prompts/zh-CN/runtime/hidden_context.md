# 本次 Gold Band 运行上下文

- 会话模式: {{ session_mode }}
- Round: {{ round_id }}
- Attempt: {{ attempt_id }}
- Attempt 目录: {{ attempt_dir }}
- 附件目录: {{ attachments_dir }}
{% if invocation_reason %}
- 调用原因: {{ invocation_reason }}
{% endif %}

{% if predecessors.is_empty %}
## 最新前序执行链
当前节点的前序运行节点：无，当前节点是本轮入口节点。
{% else %}
## 最新前序执行链
{{ predecessors.chain }}
{% endif %}

{% if predecessors.reason_lines_empty %}
{% if predecessors.is_empty %}
## 最新前序流转原因
无。
{% else %}
## 最新前序流转原因
前序节点均为普通节点，按节点结果进入当前分支。
{% endif %}
{% else %}
## 最新前序流转原因
{{ predecessors.reason_lines }}
{% endif %}

{% if not predecessors.attachment_lines_empty %}
## 最新前序附件
{{ predecessors.attachment_lines }}
{% endif %}
