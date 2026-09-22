{{ hidden_context }}

{% if continue_goal %}
# 목표
{{ continue_goal }}
{% if user_tips %}

# 사용자 팁
{{ user_tips }}
{% endif %}
{% if resume_task %}

# 작업
{{ resume_task }}
{% endif %}
{% else %}
# 요구사항
{{ requirement }}
{% if user_tips %}

# 사용자 팁
{{ user_tips }}
{% endif %}
{% if task %}

# 작업
{{ task }}
{% endif %}
{% endif %}
