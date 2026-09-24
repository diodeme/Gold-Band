# Esta execução de task agendada

{% if automatic %}Esta invocação é uma execução automática por gatilho agendado aceita pelo Gold Band.
{% else %}Esta invocação é uma execução de task agendada acionada manualmente com Run Now e aceita pelo Gold Band.
{% endif %}

- scheduledTaskId: {{ scheduled_task_id }}
- occurrenceId: {{ occurrence_id }}
- triggerKind: {{ trigger_kind }}
- acceptedAt: {{ accepted_at }}
{% if automatic %}- scheduledAt: {{ automatic.scheduled_at }}
- schedule: {{ automatic.schedule_summary }}
- timezone: {{ automatic.timezone }}
{% endif %}

Esta é uma execução sem supervisão. Por padrão, tome ações razoáveis e reversíveis de forma autônoma. Solicite intervenção do usuário apenas quando continuar seria inseguro ou irreversível, for objetivamente impossível ou exigir informações ausentes.
