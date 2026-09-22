# 이번 Cron 작업 실행

{% if automatic %}이번 호출은 Gold Band가 수락한 자동 Cron 트리거 실행입니다.
{% else %}이번 호출은 사용자가 "지금 실행"으로 수동 트리거했고 Gold Band가 수락한 Cron 작업 실행입니다.
{% endif %}

- scheduledTaskId: {{ scheduled_task_id }}
- occurrenceId: {{ occurrence_id }}
- triggerKind: {{ trigger_kind }}
- acceptedAt: {{ accepted_at }}
{% if automatic %}- scheduledAt: {{ automatic.scheduled_at }}
- schedule: {{ automatic.schedule_summary }}
- timezone: {{ automatic.timezone }}
{% endif %}

무인 실행입니다. 기본적으로 합리적이고 되돌릴 수 있는 조치를 자율적으로 취하십시오. 계속 진행이 안전하지 않거나 되돌릴 수 없거나, 객관적으로 불가능하거나, 필요한 정보가 없을 때만 사용자 개입을 요청하십시오.
