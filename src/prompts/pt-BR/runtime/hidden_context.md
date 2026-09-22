# Contexto de runtime Gold Band para esta invocação

- Session mode: {{ session_mode }}
- Round: {{ round_id }}
- Attempt: {{ attempt_id }}
- Attempt directory: {{ attempt_dir }}
- Attachments directory (local padrão para relatórios deste node, scripts temporários, notas de processo e outras saídas livres): {{ attachments_dir }}
{% if invocation_reason %}
- Invocation reason: {{ invocation_reason }}
{% endif %}

{% if predecessors.is_empty %}
## Cadeia de predecessores mais recente
Nodes executados anteriormente: nenhum. Este node é o node de entrada do round atual.
{% else %}
## Cadeia de predecessores mais recente
{{ predecessors.chain }}
{% endif %}

{% if predecessors.reason_lines_empty %}
{% if predecessors.is_empty %}
## Motivos de transição dos predecessores mais recentes
Nenhum.
{% else %}
## Motivos de transição dos predecessores mais recentes
Todos os nodes anteriores foram transições ordinárias com base no resultado do node.
{% endif %}
{% else %}
## Motivos de transição dos predecessores mais recentes
{{ predecessors.reason_lines }}
{% endif %}

{% if predecessors.has_ai_dynamic_report_manifest %}
## Manifesto completo de relatório AI-DYNAMIC (leitura sob demanda)
O `reportManifest.path` no `ai-dynamic-result` do predecessor aponta para o índice completo de relatório de execução interna, incluindo topologia de node/grupo, dependências e timing, workspaces, resumos internos e localizadores de attachment. Por padrão, use o handoff de negócio `summary`; leia o manifesto apenas quando precisar verificar execução interna, localizar attachments de relatório ou quando o `summary` não tiver o detalhe necessário.
{% endif %}

{% if not predecessors.attachment_lines_empty %}
## Attachments dos predecessores mais recentes
{{ predecessors.attachment_lines }}
{% endif %}
