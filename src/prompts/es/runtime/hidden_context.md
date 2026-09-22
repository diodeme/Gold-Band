# Contexto de runtime de Gold Band para esta invocación

- Modo de sesión: {{ session_mode }}
- Round: {{ round_id }}
- Attempt: {{ attempt_id }}
- Directorio Attempt: {{ attempt_dir }}
- Directorio de attachments (ubicación predeterminada para informes, scripts temporales, notas de proceso y otras salidas libres de este nodo): {{ attachments_dir }}
{% if invocation_reason %}
- Motivo de invocación: {{ invocation_reason }}
{% endif %}

{% if predecessors.is_empty %}
## Cadena predecesora más reciente
Nodos ejecutados anteriormente: ninguno. Este nodo es el nodo de entrada de la ronda actual.
{% else %}
## Cadena predecesora más reciente
{{ predecessors.chain }}
{% endif %}

{% if predecessors.reason_lines_empty %}
{% if predecessors.is_empty %}
## Motivos de transición predecesores más recientes
Ninguno.
{% else %}
## Motivos de transición predecesores más recientes
Todos los nodos anteriores fueron transiciones ordinarias según el resultado del nodo.
{% endif %}
{% else %}
## Motivos de transición predecesores más recientes
{{ predecessors.reason_lines }}
{% endif %}

{% if predecessors.has_ai_dynamic_report_manifest %}
## Manifiesto completo de informes AI-DYNAMIC (lectura bajo demanda)
El `reportManifest.path` en el `ai-dynamic-result` predecesor apunta al índice completo de informes de ejecución interna, incluida la topología de nodo/group, dependencias y tiempos, workspaces, resúmenes internos y localizadores de attachments. Por defecto, usa el `summary` de entrega de negocio; lee el manifiesto solo cuando necesites verificar la ejecución interna, localizar attachments de informes o el `summary` carezca del detalle requerido.
{% endif %}

{% if not predecessors.attachment_lines_empty %}
## Attachments predecesores más recientes
{{ predecessors.attachment_lines }}
{% endif %}
