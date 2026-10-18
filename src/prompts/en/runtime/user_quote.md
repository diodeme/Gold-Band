{% if quote.kind == "agentMessage" %}{% for line in quote.lines %}> {{ line }}
{% endfor %}{% elif quote.kind == "file" %}Quoted from file `{{ quote.label }}`, {% if quote.start_line == quote.end_line %}line {{ quote.start_line }}{% else %}lines {{ quote.start_line }}-{{ quote.end_line }}{% endif %}:
{{ quote.fence }}{{ quote.info }}
{{ quote.text }}
{{ quote.fence }}
{% else %}{% if quote.scope == "file" %}Full diff of `{{ quote.path }}`{% else %}Diff excerpt from `{{ quote.path }}`{% endif %} ({% if quote.origin == "workingTreeStaged" %}staged changes in the working tree{% elif quote.origin == "workingTreeUnstaged" %}unstaged changes in the working tree{% elif quote.origin == "commit" %}commit{% if quote.revision %} {{ quote.revision }}{% endif %}{% elif quote.origin == "pullRequest" %}pull request{% if quote.revision %} {{ quote.revision }}{% endif %}{% else %}changes made by the Agent in one turn of the conversation{% endif %}):
{{ quote.fence }}diff
{{ quote.text }}
{{ quote.fence }}
{% endif %}
