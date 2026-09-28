O usuário forneceu os arquivos a seguir. O conteúdo não está incluído nesta mensagem; leia-os pelo caminho quando necessário:
{% for file in files %}- `{{ file.path }}` ({{ file.size }} bytes)
{% endfor %}