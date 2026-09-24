{{ hidden_context }}

{% if continue_goal %}
# 目標
{{ continue_goal }}
{% if user_tips %}

# 使用者提示
{{ user_tips }}
{% endif %}
{% if resume_task %}

# 任務
{{ resume_task }}
{% endif %}
{% else %}
# 需求
{{ requirement }}
{% if user_tips %}

# 使用者提示
{{ user_tips }}
{% endif %}
{% if task %}

# 任務
{{ task }}
{% endif %}
{% endif %}
