Tu paso final debe emitir solo el contenido JSON del artifact `dynamic-node-completion`. No emitas explicaciones, Markdown, bloques de código ni texto adicional.

{% if agent_strategy_mode == "fixed" %}
Este nodo AI-DYNAMIC usa la estrategia fixed-agent: excepto `workflow-invocation`, todos los nodos worker, merge y acceptance internos usarán el mismo provider fijo elegido por runtime. No emitas campos provider para ningún nodo.
{{ model_policy }}
{% else %}
Este nodo AI-DYNAMIC usa la estrategia dynamic-agent: elige y emite un provider solo para los workers posteriores según la guía de enrutamiento y los providers disponibles de este prompt. Merge / acceptance siempre usan el Agent de bootstrap, así que no emitas provider para ellos. No emitas `model` ni `permissionMode` para ningún nodo; runtime lee la configuración guardada.
{{ model_policy }}
{% endif %}

El JSON Schema siguiente es el protocolo de salida efectivo de esta ejecución. Runtime lo generó a partir de las estructuras de datos Rust y lo restringió con la configuración AI-DYNAMIC actual. Tu salida debe cumplirlo; runtime usa el mismo schema para validación y diagnósticos de repair.

```json
{{ json_schema }}
```

Recordatorios de restricciones:
- Una tarea sucesora solo puede descomponer un resultado establecido dentro del alcance o reparar un `BLOCKER` calificado. No promuevas un `FOLLOW_UP` ni una sugerencia predecesora a un resultado nuevo. La deriva de alcance solo puede programar la restauración de la solución mínima dentro del alcance.
{% if agent_strategy_mode == "fixed" %}- Bajo la estrategia fixed-agent, no emitas ningún campo `provider`. Runtime inyecta automáticamente el agent fijo.
{% else %}- Bajo la estrategia dynamic-agent, los workers deben emitir un provider válido que siga la guía de enrutamiento de este prompt; `merge / acceptance` deben omitir provider porque runtime siempre usa el Agent de bootstrap.
- No emitas `provider` para `workflow-invocation`.
{% endif %}- {{ model_policy }}
- Cuando `next.type="end"`, no incluyas `node / groupId / nodes / merge / acceptance`.
{% if end_summary_is_outer_handoff %}- Si usas `next.type="end"`, `summary` debe ser una entrega de negocio completa para el sucesor fuera de AI-DYNAMIC: indica qué se completó, conclusiones clave, salidas importantes y preocupaciones restantes. No describas solo el enrutamiento ni digas “accepted”.
{% else %}- Si usas `next.type="end"`, `summary` es un informe interno de progreso o de rama. Indica con precisión qué completó este nodo para el manifiesto de informes de Runtime y el group contenedor.
{% endif %}
- Cuando `next.type="single"`, debes proporcionar un `next.node` completo, y no debes proporcionar `groupId / nodes / merge / acceptance`.
- No emitas `workspace`, un modo de workspace, una ruta ni una rama para ningún nodo. Runtime es el único responsable de la asignación de workspace.
- Un sucesor `next.type="single"` hereda automáticamente el workspace real del nodo actual.
- Si este nodo es una acceptance de group, aceptar su salida válida cierra ese group: `single` reanuda la rama de negocio original en el ámbito padre; `fanout` crea un group nuevo en el ámbito padre; solo `end` termina esa rama. Con sucesores, el group padre sigue esperando. Organiza explícitamente reparaciones y verificación; el group antiguo nunca se reabre automáticamente.
- Cuando `next.type="fanout"`, debes proporcionar juntos `groupId / nodes / merge / acceptance`, y `nodes` debe contener al menos dos ramas; usa `next.type="single"` para un solo nodo sucesor.
- Cada hijo de `next.type="fanout"` recibe automáticamente un worktree aislado; merge y acceptance vuelven automáticamente al workspace padre de ese group.
- Los worktrees hijos de fanout heredan una revision confirmada, nunca contenido sin confirmar; Runtime no hace checkpoint automáticamente. Si esta tarea tiene cambios de negocio sin confirmar necesarios para ramas sucesoras, revísalos y confírmalos por rutas concretas, opcionalmente usando Conventional Commits. Si no hay nada que confirmar, no realices operaciones Git.
- Un workspace limpio no es una puerta de fanout. Runtime da un recordatorio en la primera detección de workspace sucio; después reenvía el artifact, sin exigir un commit nuevo. No limpies el workspace, hagas stash de contenido ajeno, muevas otros worktrees, uses `git add -A` a ciegas ni cambies reglas de ignorado por este recordatorio. Deja el contenido ajeno sin cambios.
- `profile` solo está permitido en nodos worker y es opcional. Si está presente, usa un ID del enum del schema o el ID tras `profileId=...` en este prompt, no el displayName.
- No emitas `profile` para `merge` / `acceptance`; runtime usa los prompts integrados AI-DYNAMIC de merge / acceptance.
{% if agent_strategy_mode == "dynamic" %}- Si `provider` está presente, debe ser uno de los providers disponibles listados en el enum del schema o en este prompt.
{% endif %}- Si se omite `sessionMode`, se trata como `new`; usa `continue` solo al reanudar un nodo de sesión reutilizable en la cadena actual.
- Cuando `sessionMode="continue"`, debes proporcionar `continueFromNodeId`, y debe referenciar uno de los nodos de sesión reutilizables listados en este prompt.
- No uses `sessionMode="continue"` para `workflow-invocation`.
- Si `workflowId` está presente, debe ser uno de los IDs de Workflow DSL permitidos listados en el enum del schema o en este prompt.
- El recuento de nodos de fanout debe cumplir `minItems/maxItems` del schema, `maxFanout` y las restricciones de presupuesto restante mostradas en este prompt.
- Emite solo el JSON final. No emitas pseudocódigo, comentarios ni ejemplos envueltos.
