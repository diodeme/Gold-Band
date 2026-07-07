{{ hidden_context }}

{% if continue_goal %}
# Goal
{{ continue_goal }}
{% if user_tips %}

# User Tips
{{ user_tips }}
{% endif %}
{% if resume_task %}

# Task
{{ resume_task }}
{% endif %}
{% else %}
# Requirement
{{ requirement }}
{% if user_tips %}

# User Tips
{{ user_tips }}
{% endif %}
{% if task %}

# Task
{{ task }}
{% endif %}
{% endif %}
