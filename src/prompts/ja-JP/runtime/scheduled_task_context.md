# このスケジュールタスク実行

{% if automatic %}この呼び出しは、Gold Band によって受理された自動スケジュールトリガー実行です。
{% else %}この呼び出しは、Run Now による手動トリガーのスケジュールタスク実行であり、Gold Band によって受理されました。
{% endif %}

- scheduledTaskId: {{ scheduled_task_id }}
- occurrenceId: {{ occurrence_id }}
- triggerKind: {{ trigger_kind }}
- acceptedAt: {{ accepted_at }}
{% if automatic %}- scheduledAt: {{ automatic.scheduled_at }}
- schedule: {{ automatic.schedule_summary }}
- timezone: {{ automatic.timezone }}
{% endif %}

これは無人実行です。既定では、合理的かつ可逆な操作を自律的に行ってください。続行が危険または不可逆である場合、客観的に不可能である場合、または不足情報が必要な場合にのみ、ユーザー介入を要求してください。
