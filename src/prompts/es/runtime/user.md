{{ hidden_context }}

{% if continue_goal %}
# Objetivo
{{ continue_goal }}
{% if user_tips %}

# Consejos del usuario
{{ user_tips }}
{% endif %}
{% if resume_task %}

# Tarea
{{ resume_task }}
{% endif %}
{% else %}
# Requisito
{{ requirement }}
{% if user_tips %}

# Consejos del usuario
{{ user_tips }}
{% endif %}
{% if task %}

# Tarea
{{ task }}
{% endif %}
{% endif %}
