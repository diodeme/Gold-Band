## Prechequeo de identidad del requisito

Ejecuta esta sección antes de la enumeración de topología, Interview Round 0 o la enumeración de ramas Grill. Al inicio de esta sección, llama primero a `memory_read` y después realiza la comprobación de identidad.

1. Usa `memory_read` para leer la memoria de tarea y workspace actuales. Inspecciona solo las entradas `task` en la instantánea devuelta; un `storyId` o `storyName` de workspace no cuenta como identidad de tarea actual existente.
2. Continúa el Workflow del rol original solo cuando la tarea contenga tanto `storyId` como `storyName` y ambos valores sean no vacíos tras recortar espacios.
3. Si falta alguna key, está vacía o no están emparejadas, primero haz exactamente una pregunta sobre la identidad del requisito. Las opciones semánticas requeridas son:
   - `不存在`
   - `其他（用户自行输入）`
4. Usa `AskUserQuestion` o una herramienta de elicitation equivalente cuando esté disponible. De lo contrario, emite una pregunta en texto plano, pero conserva las mismas dos opciones semánticas. Esta pregunta no es un paso de topología, ronda de ambigüedad, ronda de Interview ni rama Grill. Continúa el Workflow original tras la respuesta del usuario.
5. Cuando el usuario elija `不存在`:
   - Establece `storyId` en la cadena `0`.
   - Extrae un `storyName` breve del requisito: prioriza un título explícito o un resumen de la primera línea, luego la frase nominal central del objetivo, y usa `系统需求` solo si la extracción sigue siendo imposible.
   - Elimina etiquetas de plantilla, notas de estado, marcadores Markdown y puntuación irrelevante. Guarda una línea con un máximo de 40 caracteres Unicode.
6. Cuando el usuario elija `其他（用户自行输入）`, obtén tanto el ID del requisito como el nombre del requisito. Si el texto libre no puede dividirse claramente en los dos valores, pregunta una vez más; si sigue sin quedar claro, espera aclaración en lugar de adivinar. Recorta ambos valores y guárdalos cada uno como una línea.
7. Usa `memory_write` para escribir ambas entradas en el ámbito task. Antes de escribir, usa las revisiones de las keys objetivo de la última `memory_read`: usa `operation="create"` y omite `expectedRevision` cuando la key no exista; usa `operation="update"` con la revision correspondiente al corregir una key existente. Nunca escribas estas keys en el ámbito workspace.
8. Tras ambas escrituras, llama de nuevo a `memory_read` y confirma que `storyId` y `storyName` en el ámbito task coinciden exactamente con los valores confirmados por el usuario.
9. Si `gold-band-memory` no está disponible o la lectura falla, indica que no se leyó la identidad del requisito y pregunta con normalidad según el paso 3. Si una escritura o la verificación posterior falla, indica que la identidad del requisito no se persistió. Ninguno de los dos casos es un bloqueo. Continúa con el `storyId` y `storyName` confirmados por el usuario o recién proporcionados; un nodo posterior puede volver a preguntar.
10. Nunca edites archivos de memoria directamente. Tras obtener la identidad, continúa con la inicialización de Interview o la enumeración del árbol de decisiones Grill; conserva la copia en el ámbito task cuando la escritura con herramienta tenga éxito.
