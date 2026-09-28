사용자가 다음 파일을 제공했습니다. 내용은 이 메시지에 포함되지 않았으므로 필요하면 경로로 읽으세요:
{% for file in files %}- `{{ file.path }}` ({{ file.size }}바이트)
{% endfor %}