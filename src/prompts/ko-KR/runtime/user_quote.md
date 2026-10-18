{% if quote.kind == "agentMessage" %}{% for line in quote.lines %}> {{ line }}
{% endfor %}{% elif quote.kind == "file" %}파일 `{{ quote.label }}`의 {% if quote.start_line == quote.end_line %}{{ quote.start_line }}{% else %}{{ quote.start_line }}-{{ quote.end_line }}{% endif %}번째 줄 인용:
{{ quote.fence }}{{ quote.info }}
{{ quote.text }}
{{ quote.fence }}
{% else %}{% if quote.scope == "file" %}`{{ quote.path }}`의 전체 diff{% else %}`{{ quote.path }}`의 diff 발췌{% endif %}({% if quote.origin == "workingTreeStaged" %}작업 트리의 스테이징된 변경{% elif quote.origin == "workingTreeUnstaged" %}작업 트리의 스테이징되지 않은 변경{% elif quote.origin == "commit" %}커밋{% if quote.revision %} {{ quote.revision }}{% endif %}{% elif quote.origin == "pullRequest" %}Pull Request{% if quote.revision %} {{ quote.revision }}{% endif %}{% else %}대화의 한 턴에서 Agent가 만든 변경{% endif %}):
{{ quote.fence }}diff
{{ quote.text }}
{{ quote.fence }}
{% endif %}
