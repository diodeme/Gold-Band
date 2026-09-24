{{ hidden_context }}

{% if continue_goal %}
# Objetivo
{{ continue_goal }}
{% if user_tips %}

# Dicas do usuário
{{ user_tips }}
{% endif %}
{% if resume_task %}

# Tarefa
{{ resume_task }}
{% endif %}
{% else %}
# Requisito
{{ requirement }}
{% if user_tips %}

# Dicas do usuário
{{ user_tips }}
{% endif %}
{% if task %}

# Tarefa
{{ task }}
{% endif %}
{% endif %}
