{{ hidden_context }}

{% if continue_goal %}
# Goal
{{ continue_goal }}
{% else %}
# Requirement
{{ requirement }}
{% if task %}

# Task
{{ task }}
{% endif %}
{% endif %}
