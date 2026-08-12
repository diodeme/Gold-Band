# Scheduled Task Execution Context

The current session is triggered automatically by a scheduled task. Key context:

- Task name: {{ scheduled_title }}
- Execution mode: {{ scheduled_mode }}
- Session policy: {{ scheduled_session_policy }}
- Trigger type: {{ scheduled_trigger_kind }}
- Trigger time: {{ scheduled_triggered_at }}
{% if scheduled_instruction %}
- Scheduled task instruction: {{ scheduled_instruction }}
{% endif %}

**Note**: You are executing in a scheduled task environment where the user may not be present. Complete the task autonomously. If you encounter an issue that cannot be resolved automatically, clearly describe the obstacle and the specific reason user intervention is required.
