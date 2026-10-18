{% if quote.kind == "agentMessage" %}{% for line in quote.lines %}> {{ line }}
{% endfor %}{% elif quote.kind == "file" %}Fragmento del archivo `{{ quote.label }}`, {% if quote.start_line == quote.end_line %}línea {{ quote.start_line }}{% else %}líneas {{ quote.start_line }}-{{ quote.end_line }}{% endif %}:
{{ quote.fence }}{{ quote.info }}
{{ quote.text }}
{{ quote.fence }}
{% else %}{% if quote.scope == "file" %}Diff completo de `{{ quote.path }}`{% else %}Fragmento del diff de `{{ quote.path }}`{% endif %} ({% if quote.origin == "workingTreeStaged" %}cambios preparados en el árbol de trabajo{% elif quote.origin == "workingTreeUnstaged" %}cambios sin preparar en el árbol de trabajo{% elif quote.origin == "commit" %}commit{% if quote.revision %} {{ quote.revision }}{% endif %}{% elif quote.origin == "pullRequest" %}pull request{% if quote.revision %} {{ quote.revision }}{% endif %}{% else %}cambios hechos por el Agent en un turno de la conversación{% endif %}):
{{ quote.fence }}diff
{{ quote.text }}
{{ quote.fence }}
{% endif %}
