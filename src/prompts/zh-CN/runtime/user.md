{{ hidden_context }}

{% if continue_goal %}
# Goal
{{ continue_goal }}
{% if resume_task %}

# Task
{{ resume_task }}
{% endif %}
{% else %}
# Requirement
{{ requirement }}
{% if task %}

# Task
{{ task }}
{% endif %}
{% endif %}
