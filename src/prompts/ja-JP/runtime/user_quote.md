{% if quote.kind == "agentMessage" %}{% for line in quote.lines %}> {{ line }}
{% endfor %}{% elif quote.kind == "file" %}ファイル `{{ quote.label }}` の {% if quote.start_line == quote.end_line %}{{ quote.start_line }}{% else %}{{ quote.start_line }}-{{ quote.end_line }}{% endif %} 行目からの引用：
{{ quote.fence }}{{ quote.info }}
{{ quote.text }}
{{ quote.fence }}
{% else %}{% if quote.scope == "file" %}`{{ quote.path }}` の diff 全体{% else %}`{{ quote.path }}` の diff からの抜粋{% endif %}（{% if quote.origin == "workingTreeStaged" %}作業ツリーでステージ済みの変更{% elif quote.origin == "workingTreeUnstaged" %}作業ツリーで未ステージの変更{% elif quote.origin == "commit" %}コミット{% if quote.revision %} {{ quote.revision }}{% endif %}{% elif quote.origin == "pullRequest" %}Pull Request{% if quote.revision %} {{ quote.revision }}{% endif %}{% else %}会話のあるターンで Agent が行った変更{% endif %}）：
{{ quote.fence }}diff
{{ quote.text }}
{{ quote.fence }}
{% endif %}
