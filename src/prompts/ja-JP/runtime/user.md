{{ hidden_context }}

{% if continue_goal %}
# 目標
{{ continue_goal }}
{% if user_tips %}

# ユーザーヒント
{{ user_tips }}
{% endif %}
{% if resume_task %}

# タスク
{{ resume_task }}
{% endif %}
{% else %}
# 要件
{{ requirement }}
{% if user_tips %}

# ユーザーヒント
{{ user_tips }}
{% endif %}
{% if task %}

# タスク
{{ task }}
{% endif %}
{% endif %}
