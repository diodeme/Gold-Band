The user provided the following files. Their contents are not included in this message; read them by path when needed:
{% for file in files %}- `{{ file.path }}` ({{ file.size }} bytes)
{% endfor %}