## Auto-commit de desarrollo y pruebas

Tras completar la implementación del requisito y las pruebas automatizadas requeridas, ejecuta este paso de cierre antes de terminar el nodo.

1. Usa `memory_read` para inspeccionar `storyId` y `storyName` en el ámbito task. Reutiliza ambos cuando sean no vacíos. Cuando falte alguno, esté vacío o no estén emparejados, genera primero la identidad con `storyId=0` y las reglas de extracción del nombre del requisito e intenta `memory_write`. Para una key ausente usa `operation="create"` y omite `expectedRevision`; para una key existente usa `operation="update"` con su revision. Si la herramienta no está disponible o la lectura falla, indica que no se leyó la identidad; si la escritura o la verificación fallan, indica que no se persistió. Ninguno de los dos casos es un bloqueo: pregunta al usuario la identidad del requisito o continúa con la identidad confirmada en esta ejecución.
2. Identifica los cambios relacionados con la tarea producidos por este nodo, incluidos código, pruebas, prompts, documentos de diseño del producto y el plan de desarrollo. Excluye cambios que existían antes de la ejecución o cambios del usuario claramente no relacionados.
3. Si no hay cambios relacionados con la tarea, no crees un commit vacío. Registra que no se requirió commit, la evidencia y cualquier cambio sin confirmar retenido que no se incluyó.
4. Selecciona un token de tipo Conventional Commits estándar y una descripción concisa en chino a partir del diff real relacionado con la tarea. Céntrate en el resultado entregado, no en el historial de ejecución, nombres de modelos ni afirmaciones genéricas.
5. Ejecuta `git add` solo con rutas concretas. Nunca uses `git add -A` ni incluyas cambios no relacionados. Crea un commit lógico tras una ejecución exitosa del nodo.
6. El mensaje de commit debe contener exactamente tres líneas:

```text
--story=[{storyId}] {storyName}
{type}: {中文描述}
#AI COMMIT#
```

7. Tras el commit, verifica el OID del commit, el mensaje completo del commit y el estado de las rutas relacionadas con la tarea. Nunca declares completado el nodo cuando el commit o la verificación hayan fallado.
8. Registra el OID del commit, el mensaje de tres líneas, las rutas incluidas, las rutas excluidas y los motivos en el informe de desarrollo y pruebas. CI/CD usa estos OID de commit para determinar si esta tarea tiene commits sin enviar.
9. No pidas al usuario que confirme el mensaje o el formato del commit. Las reglas existentes de permisos de comandos ACP siguen vigentes; nunca eludas los límites de permiso por auto-commit.
10. Si el commit falla, conserva el error original y el estado Git actual para la ruta existente de fallo de nodo, recuperación manual o fallo de aceptación. No crees un commit sustituto ni reescribas el historial de commits.
