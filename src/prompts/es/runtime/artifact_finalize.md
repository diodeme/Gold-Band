Este turno de respuesta terminó, pero runtime aún no ha recibido el artifact de este nodo. El protocolo siguiente se proporciona o reitera; no significa que la tarea esté completa ni exige un cierre anticipado. Decide si pretendes finalizar este nodo.

- Si no pretendías finalizar este nodo, continúa ejecutando directamente dentro del alcance de tarea, workspace y permisos de herramientas actuales. No se requiere etiqueta de estado, y no entregues anticipadamente solo para responder a este prompt. Tras continuar, emite el artifact siguiente cuando consideres finalizado este nodo.
- Si consideras finalizado este nodo, emite el artifact siguiente según el trabajo completado. No vuelvas a auditar los objetivos de la tarea ni los requisitos de aceptación, ni añadas trabajo de negocio por este prompt.
- Antes de emitir el artifact, si la tarea actual requiere un informe u otro adjunto y aún no se ha escrito, escríbelo en el directorio attachments del intent actual; omite este paso si es innecesario o ya está completo.
{% if can_read_runtime_snapshot %}- Al preparar el artifact, actualiza una instantánea de runtime de solo lectura solo cuando el contexto de runtime siguiente lo exija explícitamente; lee únicamente la ruta de instantánea declarada.
{% endif %}- Mientras continúas la ejecución, responde y usa herramientas con normalidad. Solo la salida final del artifact debe omitir explicaciones, Markdown y bloques de código.
{% if finalize_context %}
El contexto de runtime siguiente es solo para esta normalización del resultado de control:
{{ finalize_context }}
{% endif %}

Artifact de salida: {{ artifact }}
Tipo de salida: {{ kind }}

Sigue este protocolo solo cuando consideres finalizado este nodo y decidas emitir el artifact:
{{ schema }}{% if success_condition %}

runtime evaluará posteriormente el resultado del nodo con esta condición:
{{ success_condition }}{% endif %}
