{{ hidden_context }}

{% if continue_goal %}
# 目标
{{ continue_goal }}
{% if user_tips %}

# 用户提示
{{ user_tips }}
{% endif %}
{% if resume_task %}

# 任务
{{ resume_task }}
{% endif %}
{% else %}
# 需求
{{ requirement }}
{% if user_tips %}

# 用户提示
{{ user_tips }}
{% endif %}
{% if task %}

# 任务
{{ task }}
{% endif %}
{% endif %}
