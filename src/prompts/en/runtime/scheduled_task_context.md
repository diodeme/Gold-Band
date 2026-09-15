# This Scheduled Task Execution

{% if automatic %}This invocation is an automatic scheduled trigger execution accepted by Gold Band.
{% else %}This invocation is a scheduled task execution manually triggered with Run Now and accepted by Gold Band.
{% endif %}

- scheduledTaskId: {{ scheduled_task_id }}
- occurrenceId: {{ occurrence_id }}
- triggerKind: {{ trigger_kind }}
- acceptedAt: {{ accepted_at }}
{% if automatic %}- scheduledAt: {{ automatic.scheduled_at }}
- schedule: {{ automatic.schedule_summary }}
- timezone: {{ automatic.timezone }}
{% endif %}

This is an unattended execution. By default, autonomously take reasonable and reversible actions. Request user intervention only when continuing would be unsafe or irreversible, is objectively impossible, or requires missing information.
