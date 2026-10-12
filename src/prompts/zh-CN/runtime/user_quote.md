{% if quote.kind == "agentMessage" %}{% for line in quote.lines %}> {{ line }}
{% endfor %}{% elif quote.kind == "file" %}引用自文件 `{{ quote.label }}` 第 {% if quote.start_line == quote.end_line %}{{ quote.start_line }}{% else %}{{ quote.start_line }}-{{ quote.end_line }}{% endif %} 行：
{{ quote.fence }}{{ quote.info }}
{{ quote.text }}
{{ quote.fence }}
{% else %}{% if quote.scope == "file" %}`{{ quote.path }}` 的完整 diff{% else %}引用自 `{{ quote.path }}` 的 diff 片段{% endif %}（{% if quote.origin == "workingTreeStaged" %}工作区已暂存的改动{% elif quote.origin == "workingTreeUnstaged" %}工作区未暂存的改动{% elif quote.origin == "commit" %}提交{% if quote.revision %} {{ quote.revision }}{% endif %}{% elif quote.origin == "pullRequest" %}Pull Request{% if quote.revision %} {{ quote.revision }}{% endif %}{% else %}Agent 在会话中某一轮产生的改动{% endif %}）：
{{ quote.fence }}diff
{{ quote.text }}
{{ quote.fence }}
{% endif %}
