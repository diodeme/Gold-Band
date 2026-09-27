{% if quote.kind == "agentMessage" %}{% for line in quote.lines %}> {{ line }}
{% endfor %}{% elif quote.kind == "file" %}Trecho do arquivo `{{ quote.label }}`, {% if quote.start_line == quote.end_line %}linha {{ quote.start_line }}{% else %}linhas {{ quote.start_line }}-{{ quote.end_line }}{% endif %}:
{{ quote.fence }}{{ quote.info }}
{{ quote.text }}
{{ quote.fence }}
{% else %}{% if quote.scope == "file" %}Diff completo de `{{ quote.path }}`{% else %}Trecho do diff de `{{ quote.path }}`{% endif %} ({% if quote.origin == "workingTreeStaged" %}alterações preparadas na árvore de trabalho{% elif quote.origin == "workingTreeUnstaged" %}alterações não preparadas na árvore de trabalho{% elif quote.origin == "commit" %}commit{% if quote.revision %} {{ quote.revision }}{% endif %}{% elif quote.origin == "pullRequest" %}pull request{% if quote.revision %} {{ quote.revision }}{% endif %}{% else %}alterações feitas pelo Agent em um turno da conversa{% endif %}):
{{ quote.fence }}diff
{{ quote.text }}
{{ quote.fence }}
{% endif %}
