# Contexto de runtime AI-DYNAMIC para esta invocación

## Nodo dinámico actual
- Nodo padre: {{ outer_node_id }}
- Attempt padre: {{ outer_attempt_id }}
- Dynamic run: {{ dynamic_run_id }}
- Nodo interno: {{ node_id }}
- Título: {{ title }}
- Tipo: {{ kind }}
- Group: {{ group_id }}
- Chain: {{ chain_id }}
- Profundidad: {{ depth }}

## Ubicación de runtime
- Raíz Dynamic: {{ dynamic_root }}
- Nodo interno (relativo a la raíz Dynamic): {{ node_dir }}
- Attempt actual (relativo al nodo interno): {{ attempt_dir }}
- Attachments (relativo al attempt actual): {{ attachments_dir }}
- Workspace ID: {{ workspace_id }}
- Ruta Workspace: {{ workspace_path }}
- Capacidad Workspace:
{{ workspace_capability }}

{% if has_new_round_trigger %}
## Retroalimentación del activador `$new-round`
{{ new_round_trigger }}
- Esta es la salida fallida del nodo que abrió la Round nueva actual. Comprende su motivo de fallo y el trabajo pendiente antes de planificar las tareas internas de esta Round; no repitas simplemente el requisito original sin cambios.
- La vista previa del artifact puede estar truncada. Lee el artifact o los attachments listados explícitamente cuando se necesiten detalles completos.
{% endif %}

{% if has_coordination_snapshot %}
## Instantánea de coordinación de Runtime
- Instantánea de solo lectura (relativa a la raíz Dynamic): {{ coordination_snapshot_path }}
- Runtime la deriva del grafo dynamic canónico y es su único escritor. No la modifiques.
- Lee la instantánea más reciente antes de iniciar o continuar esta tarea: usa el objetivo, el estado TODO, la relación padre-hijo y los steps de cada `workstreams[]` para entender otras subtareas; luego usa la anidación y la phase de `groups[]` para evitar trabajo duplicado o conflictivo.
- Lee de nuevo la misma ruta antes de emitir `next.type="single"` o `next.type="fanout"`, y planifica sucesores a partir del estado más reciente.
{% endif %}

{% if has_direct_predecessors %}
## Predecesores directos
{{ direct_predecessors }}
{% endif %}

{% if has_active_group %}
## Group activo
{{ active_group }}
{% endif %}

{% if has_inherited_groups %}
## Contexto de group heredado
{{ inherited_groups }}
{% endif %}

{% if has_siblings %}
## Nodos hermanos en paralelo
{{ siblings }}
{% endif %}

{% if has_available_attachments %}
## Attachments disponibles
- Solo se listan rutas de attachments; no se lee ni se incluye inline el contenido de los attachments. Forma la ruta completa de una entrada regular uniendo la `Raíz Dynamic` con sus niveles sucesivos del árbol de rutas; una entrada de nivel superior `absolutePath=` ya es completa y debe usarse tal cual.
{% if has_predecessor_attachments %}
### Cadena predecesora (cadena de entrega de tareas que creó el nodo actual; hasta {{ source_predecessor_limit }} nodos)
{{ predecessor_attachments }}
{% if has_predecessor_attachment_overflow %}
- Los listados de attachments de los nodos origen siguientes están truncados o incompletos. Como máximo se inspeccionan {{ attachments_per_source_limit }} archivos o directorios vacíos por nodo; los directorios no vacíos se recorren y no consumen una ranura por sí solos. Arriba solo se listan archivos encontrados; inspecciona los directorios attachments completos según sea necesario:
{{ predecessor_attachment_overflow_directories }}
{% endif %}
{% endif %}
{% if has_dependency_attachments %}
### Dependencias explícitas (nodos de entrada nombrados explícitamente por el nodo actual mediante dependsOn)
{{ dependency_attachments }}
{% if has_dependency_attachment_overflow %}
- Los listados de attachments de los nodos origen siguientes están truncados o incompletos. Como máximo se inspeccionan {{ attachments_per_source_limit }} archivos o directorios vacíos por nodo; los directorios no vacíos se recorren y no consumen una ranura por sí solos. Arriba solo se listan archivos encontrados; inspecciona los directorios attachments completos según sea necesario:
{{ dependency_attachment_overflow_directories }}
{% endif %}
{% endif %}
{% if has_group_evidence_attachments %}
### Evidencia de group (entradas actuales de merge / acceptance o el merge y acceptance más recientes de groups relacionados)
{{ group_evidence_attachments }}
{% if has_group_evidence_attachment_overflow %}
- Los listados de attachments de los nodos origen siguientes están truncados o incompletos. Como máximo se inspeccionan {{ attachments_per_source_limit }} archivos o directorios vacíos por nodo; los directorios no vacíos se recorren y no consumen una ranura por sí solos. Arriba solo se listan archivos encontrados; inspecciona los directorios attachments completos según sea necesario:
{{ group_evidence_attachment_overflow_directories }}
{% endif %}
{% endif %}
{% endif %}

{% if has_output_contract %}
## Reutilización de sesión
- Modo de sesión: {{ session_mode }}
- continueFromNodeId: {{ continue_from_node_id }}
- Nota: `continue` solo reutiliza el contexto de sesión ACP del nodo origen; la tarea actual es la `# Tarea` de este user prompt.
- Nodos de sesión reutilizables en la cadena actual:
{{ resumable_sessions }}

## Límites de runtime
- Allowed workflow snapshots:
{{ allowed_workflow_snapshots }}
- Presupuesto restante:
{{ remaining_budget }}

## Opciones de Agent y profile
- Estrategia de agent del nodo dinámico: {{ agent_strategy_mode }}
- Agent de bootstrap: {{ bootstrap_provider }}
{% if agent_strategy_mode == "dynamic" %}- Guía de enrutamiento de Agent:
{{ agent_routing_prompt }}
- Política de modelo merge / acceptance:
{{ acceptance_model_policy }}
{% endif %}- Agents disponibles y opciones de runtime configuradas:
{{ available_providers }}
- Profiles disponibles:
{{ available_profiles }}
{% endif %}
