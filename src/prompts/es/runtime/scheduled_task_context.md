# Esta ejecución de tarea programada

{% if automatic %}Esta invocación es una ejecución por activación programada automática aceptada por Gold Band.
{% else %}Esta invocación es una ejecución de tarea programada activada manualmente con Run Now y aceptada por Gold Band.
{% endif %}

- scheduledTaskId: {{ scheduled_task_id }}
- occurrenceId: {{ occurrence_id }}
- triggerKind: {{ trigger_kind }}
- acceptedAt: {{ accepted_at }}
{% if automatic %}- scheduledAt: {{ automatic.scheduled_at }}
- schedule: {{ automatic.schedule_summary }}
- timezone: {{ automatic.timezone }}
{% endif %}

Esta es una ejecución sin supervisión. Por defecto, toma de forma autónoma acciones razonables y reversibles. Solicita intervención del usuario solo cuando continuar sería inseguro o irreversible, sea objetivamente imposible o falte información necesaria.
