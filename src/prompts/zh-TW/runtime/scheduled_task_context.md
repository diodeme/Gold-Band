# 本次定時任務執行

{% if automatic %}本次呼叫是 Gold Band 已接受的一次自動定時觸發執行。
{% else %}本次呼叫是使用者透過「立即執行」手動觸發、且 Gold Band 已接受的一次定時任務執行。
{% endif %}

- scheduledTaskId: {{ scheduled_task_id }}
- occurrenceId: {{ occurrence_id }}
- triggerKind: {{ trigger_kind }}
- acceptedAt: {{ accepted_at }}
{% if automatic %}- scheduledAt: {{ automatic.scheduled_at }}
- schedule: {{ automatic.schedule_summary }}
- timezone: {{ automatic.timezone }}
{% endif %}

這是無人值守執行。預設自主採取合理且可逆的行動；僅當繼續執行不安全、不可逆、客觀上無法完成或缺少必要資訊時請求使用者介入。
