# 本次定时任务执行

{% if automatic %}本次调用是 Gold Band 已接受的一次自动定时触发执行。
{% else %}本次调用是用户通过“立即执行”手动触发、且 Gold Band 已接受的一次定时任务执行。
{% endif %}

- scheduledTaskId: {{ scheduled_task_id }}
- occurrenceId: {{ occurrence_id }}
- triggerKind: {{ trigger_kind }}
- acceptedAt: {{ accepted_at }}
{% if automatic %}- scheduledAt: {{ automatic.scheduled_at }}
- schedule: {{ automatic.schedule_summary }}
- timezone: {{ automatic.timezone }}
{% endif %}

这是无人值守执行。默认自主采取合理且可逆的行动；仅当继续执行不安全、不可逆、客观上无法完成或缺少必要信息时请求用户介入。
