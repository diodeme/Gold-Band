La proposal `dynamic-node-completion` anterior no ha sido aceptada. Aborda los elementos de validación o el recordatorio siguientes y vuelve a enviarla.

Debes reparar la salida final `dynamic-node-completion` para que cumpla las restricciones de runtime siguientes.
Repara solo errores de validación de protocolo; no vuelvas a ejecutar la tarea. El trabajo sucesor debe seguir cumpliendo el contrato de alcance; elimina o reduce los elementos fuera de alcance.
{% if fanout_workspace_dirty %}
Este fanout está a punto de crear worktrees desde HEAD. Se detectó código sin confirmar en el workspace origen, así que ten en cuenta:
- Workspace origen de bifurcación: {{ fanout_workspace_path }}. Comprueba si esta tarea tiene cambios de negocio sin confirmar necesarios para las ramas sucesoras. Si los hay, revísalos y confírmalos por rutas concretas, opcionalmente usando Conventional Commits.
- Si no hay nada que confirmar, no realices operaciones Git y reenvía directamente el artifact. No se exige un workspace limpio ni un commit nuevo; los archivos sucios restantes no volverán a bloquear el fanout.
- No limpies el workspace, hagas stash de contenido ajeno, muevas otros worktrees, uses `git add -A` a ciegas ni cambies reglas de ignorado por este recordatorio. Conserva contenido ajeno y cambios que no puedan gestionarse con seguridad.
- El artifact reenviado debe seguir pasando el resto de validaciones de protocolo. No cambies el schema ni añadas campos de workspace/branch.
{% endif %}
No emitas explicaciones, Markdown, bloques de código ni texto adicional. Emite solo el contenido reparado de `dynamic-node-completion`.

{% if has_coordination_snapshot %}Instantánea de coordinación más reciente:
- Instantánea de solo lectura: {{ coordination_snapshot_path }}
- Lee la instantánea de coordinación más reciente antes de reparar y emitir `next.type="single"` o `next.type="fanout"`; solo lectura, no modifiques este archivo.
{% endif %}

Errores de validación:
{{ validation_errors }}

Referencia de valores válidos actuales:
{{ repair_reference }}

Presupuesto restante actual:
{{ remaining_budget }}
